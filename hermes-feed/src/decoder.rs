use std::collections::HashSet;
use std::time::Instant;

use alloy_consensus::transaction::SignerRecoverable;
use alloy_consensus::{Transaction, TxEnvelope};
use alloy_eips::eip2718::Decodable2718;
use alloy_primitives::{Address, B256};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::feed::{BroadcastFeedMessage, BroadcastMessage};
use crate::uniswap_v2::{V2SwapIntent, decode_v2_exact_input};

const L1_MESSAGE_TYPE_L2_MESSAGE: u8 = 3;
const L2_MESSAGE_KIND_BATCH: u8 = 3;
const L2_MESSAGE_KIND_SIGNED_TX: u8 = 4;
const MAX_L2_MESSAGE_SIZE: usize = 256 * 1024;
const MAX_BATCH_DEPTH: usize = 16;

#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub routers: HashSet<Address>,
    pub selectors: HashSet<[u8; 4]>,
    pub watched_wallets: HashSet<Address>,
    pub emit_transaction_hashes: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Candidate {
    pub sequence_number: u64,
    pub l1_block_number: u64,
    pub l1_timestamp: u64,
    pub tx_hash: B256,
    pub from: Address,
    pub to: Address,
    pub selector: Option<[u8; 4]>,
    pub v2_swap: Option<V2SwapIntent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionFingerprint {
    pub sequence_number: u64,
    pub tx_hash: B256,
}

#[derive(Debug, Clone, Copy)]
pub struct TransactionContext<'a> {
    pub sequence_number: u64,
    pub l1_block_number: u64,
    pub l1_timestamp: u64,
    pub transaction: &'a TxEnvelope,
}

#[derive(Debug, Clone, Default)]
pub struct DecodeReport {
    pub messages: usize,
    pub signed_transactions: usize,
    pub router_matches: usize,
    pub selector_matches: usize,
    pub recovered_signers: usize,
    pub transaction_fingerprints: Vec<TransactionFingerprint>,
    pub candidates: Vec<Candidate>,
    pub unsupported_l1_messages: usize,
    pub unsupported_l2_messages: usize,
    pub base64_ns: u64,
    pub l2_walk_ns: u64,
    pub envelope_decode_ns: u64,
    pub filter_ns: u64,
}

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unsupported feed version {0}")]
    UnsupportedFeedVersion(u64),
    #[error("invalid base64 l2Msg at sequence {sequence}: {source}")]
    Base64 {
        sequence: u64,
        #[source]
        source: base64::DecodeSliceError,
    },
    #[error("L2 message at sequence {sequence} exceeds {MAX_L2_MESSAGE_SIZE} bytes")]
    MessageTooLarge { sequence: u64 },
    #[error("empty L2 message at sequence {sequence}")]
    EmptyL2Message { sequence: u64 },
    #[error("batch nesting exceeds {MAX_BATCH_DEPTH} at sequence {sequence}")]
    BatchTooDeep { sequence: u64 },
    #[error("truncated batch length at sequence {sequence}")]
    TruncatedBatchLength { sequence: u64 },
    #[error("batch child length {length} is invalid at sequence {sequence}")]
    InvalidBatchLength { sequence: u64, length: usize },
    #[error("invalid signed EIP-2718 transaction at sequence {sequence}: {reason}")]
    InvalidTransaction { sequence: u64, reason: String },
    #[error("could not recover signer at sequence {sequence}: {reason}")]
    SignerRecovery { sequence: u64, reason: String },
}

pub struct FeedDecoder {
    filter: Filter,
    base64_buffer: Vec<u8>,
}

impl FeedDecoder {
    pub fn new(filter: Filter) -> Self {
        Self {
            filter,
            base64_buffer: Vec::with_capacity(64 * 1024),
        }
    }

    pub fn decode(&mut self, feed: &BroadcastMessage) -> Result<DecodeReport, DecodeError> {
        self.decode_with(feed, |_| {})
    }

    /// Decode and visit transactions. The visitor must remain side-effect-free:
    /// a later malformed child can still make the containing batch fail. Act on
    /// collected values only after this method returns `Ok`.
    pub fn decode_with<F>(
        &mut self,
        feed: &BroadcastMessage,
        mut visitor: F,
    ) -> Result<DecodeReport, DecodeError>
    where
        F: for<'a> FnMut(TransactionContext<'a>),
    {
        if feed.version != 1 {
            return Err(DecodeError::UnsupportedFeedVersion(feed.version));
        }

        let mut report = DecodeReport {
            messages: feed.messages.len(),
            ..DecodeReport::default()
        };
        for message in &feed.messages {
            self.decode_message_inner(message, &mut report, &mut visitor)?;
        }
        Ok(report)
    }

    /// Decode one feed message. As with [`Self::decode_with`], defer external
    /// effects until the method has returned successfully.
    pub fn decode_message_with<F>(
        &mut self,
        feed_message: &BroadcastFeedMessage,
        mut visitor: F,
    ) -> Result<DecodeReport, DecodeError>
    where
        F: for<'a> FnMut(TransactionContext<'a>),
    {
        let mut report = DecodeReport {
            messages: 1,
            ..DecodeReport::default()
        };
        self.decode_message_inner(feed_message, &mut report, &mut visitor)?;
        Ok(report)
    }

    fn decode_message_inner<F>(
        &mut self,
        feed_message: &BroadcastFeedMessage,
        report: &mut DecodeReport,
        visitor: &mut F,
    ) -> Result<(), DecodeError>
    where
        F: for<'a> FnMut(TransactionContext<'a>),
    {
        let incoming = &feed_message.message.message;
        if incoming.header.kind != L1_MESSAGE_TYPE_L2_MESSAGE {
            report.unsupported_l1_messages += 1;
            return Ok(());
        }

        let started = Instant::now();
        let estimated = incoming.l2_msg.len().saturating_mul(3) / 4 + 3;
        self.base64_buffer.resize(estimated, 0);
        let decoded_len = STANDARD
            .decode_slice(incoming.l2_msg.as_bytes(), &mut self.base64_buffer)
            .map_err(|source| DecodeError::Base64 {
                sequence: feed_message.sequence_number,
                source,
            })?;
        self.base64_buffer.truncate(decoded_len);
        report.base64_ns = report.base64_ns.saturating_add(elapsed_ns(started));

        if self.base64_buffer.len() > MAX_L2_MESSAGE_SIZE {
            return Err(DecodeError::MessageTooLarge {
                sequence: feed_message.sequence_number,
            });
        }

        let started = Instant::now();
        Self::decode_l2(
            &self.filter,
            &self.base64_buffer,
            feed_message,
            0,
            report,
            visitor,
        )?;
        report.l2_walk_ns = report.l2_walk_ns.saturating_add(elapsed_ns(started));
        Ok(())
    }

    fn decode_l2<F>(
        filter: &Filter,
        bytes: &[u8],
        feed_message: &BroadcastFeedMessage,
        depth: usize,
        report: &mut DecodeReport,
        visitor: &mut F,
    ) -> Result<(), DecodeError>
    where
        F: for<'a> FnMut(TransactionContext<'a>),
    {
        let Some((&kind, payload)) = bytes.split_first() else {
            return Err(DecodeError::EmptyL2Message {
                sequence: feed_message.sequence_number,
            });
        };

        match kind {
            L2_MESSAGE_KIND_SIGNED_TX => {
                Self::decode_signed(filter, payload, feed_message, report, visitor)
            }
            L2_MESSAGE_KIND_BATCH => {
                if depth >= MAX_BATCH_DEPTH {
                    return Err(DecodeError::BatchTooDeep {
                        sequence: feed_message.sequence_number,
                    });
                }
                Self::decode_batch(filter, payload, feed_message, depth + 1, report, visitor)
            }
            _ => {
                report.unsupported_l2_messages += 1;
                Ok(())
            }
        }
    }

    fn decode_batch<F>(
        filter: &Filter,
        mut payload: &[u8],
        feed_message: &BroadcastFeedMessage,
        depth: usize,
        report: &mut DecodeReport,
        visitor: &mut F,
    ) -> Result<(), DecodeError>
    where
        F: for<'a> FnMut(TransactionContext<'a>),
    {
        while !payload.is_empty() {
            if payload.len() < 8 {
                return Err(DecodeError::TruncatedBatchLength {
                    sequence: feed_message.sequence_number,
                });
            }
            let length = u64::from_be_bytes(payload[..8].try_into().expect("eight bytes")) as usize;
            payload = &payload[8..];
            if length == 0 || length > MAX_L2_MESSAGE_SIZE || length > payload.len() {
                return Err(DecodeError::InvalidBatchLength {
                    sequence: feed_message.sequence_number,
                    length,
                });
            }
            let (child, remaining) = payload.split_at(length);
            Self::decode_l2(filter, child, feed_message, depth, report, visitor)?;
            payload = remaining;
        }
        Ok(())
    }

    fn decode_signed<F>(
        filter: &Filter,
        payload: &[u8],
        feed_message: &BroadcastFeedMessage,
        report: &mut DecodeReport,
        visitor: &mut F,
    ) -> Result<(), DecodeError>
    where
        F: for<'a> FnMut(TransactionContext<'a>),
    {
        let started = Instant::now();
        let tx = TxEnvelope::decode_2718_exact(payload).map_err(|error| {
            DecodeError::InvalidTransaction {
                sequence: feed_message.sequence_number,
                reason: error.to_string(),
            }
        })?;
        if matches!(&tx, TxEnvelope::Eip4844(_)) {
            return Err(DecodeError::InvalidTransaction {
                sequence: feed_message.sequence_number,
                reason: "EIP-4844 blob transactions are unsupported by Nitro".into(),
            });
        }
        report.envelope_decode_ns = report
            .envelope_decode_ns
            .saturating_add(elapsed_ns(started));
        report.signed_transactions += 1;
        if filter.emit_transaction_hashes {
            report
                .transaction_fingerprints
                .push(TransactionFingerprint {
                    sequence_number: feed_message.sequence_number,
                    tx_hash: *tx.tx_hash(),
                });
        }

        visitor(TransactionContext {
            sequence_number: feed_message.sequence_number,
            l1_block_number: feed_message.message.message.header.block_number,
            l1_timestamp: feed_message.message.message.header.timestamp,
            transaction: &tx,
        });

        // An empty router allowlist is decode-only mode. It deliberately avoids
        // recovering every signer on the chain.
        if filter.routers.is_empty() {
            return Ok(());
        }

        let started = Instant::now();
        let Some(to) = tx.to() else {
            report.filter_ns = report.filter_ns.saturating_add(elapsed_ns(started));
            return Ok(());
        };
        if !filter.routers.contains(&to) {
            report.filter_ns = report.filter_ns.saturating_add(elapsed_ns(started));
            return Ok(());
        }
        report.router_matches += 1;

        let selector = tx.input().get(..4).map(|bytes| {
            let mut value = [0_u8; 4];
            value.copy_from_slice(bytes);
            value
        });
        if !filter.selectors.is_empty()
            && !selector.is_some_and(|selector| filter.selectors.contains(&selector))
        {
            report.filter_ns = report.filter_ns.saturating_add(elapsed_ns(started));
            return Ok(());
        }
        report.selector_matches += 1;

        let from = tx
            .recover_signer()
            .map_err(|error| DecodeError::SignerRecovery {
                sequence: feed_message.sequence_number,
                reason: error.to_string(),
            })?;
        report.recovered_signers += 1;
        if !filter.watched_wallets.is_empty() && !filter.watched_wallets.contains(&from) {
            report.filter_ns = report.filter_ns.saturating_add(elapsed_ns(started));
            return Ok(());
        }

        report.candidates.push(Candidate {
            sequence_number: feed_message.sequence_number,
            l1_block_number: feed_message.message.message.header.block_number,
            l1_timestamp: feed_message.message.message.header.timestamp,
            tx_hash: *tx.tx_hash(),
            from,
            to,
            selector,
            v2_swap: decode_v2_exact_input(tx.input(), tx.value()),
        });
        report.filter_ns = report.filter_ns.saturating_add(elapsed_ns(started));
        Ok(())
    }
}

fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy_primitives::Address;
    use base64::Engine;
    use serde_json::json;

    use super::*;
    use crate::feed::BroadcastMessage;

    const LEGACY_TX: &str = "df800182520894000000000000000000000000000000000000000080801b0101";

    fn feed_with_l2(l2: &[u8]) -> BroadcastMessage {
        serde_json::from_value(json!({
            "version": 1,
            "messages": [{
                "sequenceNumber": 42,
                "message": {"message": {
                    "header": {"kind": 3, "blockNumber": 9, "timestamp": 10},
                    "l2Msg": base64::engine::general_purpose::STANDARD.encode(l2)
                }}
            }]
        }))
        .unwrap()
    }

    #[test]
    fn filters_destination_before_recovering_signer() {
        let raw = hex::decode(LEGACY_TX).unwrap();
        let mut l2 = vec![L2_MESSAGE_KIND_SIGNED_TX];
        l2.extend_from_slice(&raw);
        let mut filter = Filter::default();
        filter
            .routers
            .insert(Address::from_str("0x1111111111111111111111111111111111111111").unwrap());
        let mut decoder = FeedDecoder::new(filter);
        let report = decoder.decode(&feed_with_l2(&l2)).unwrap();

        assert_eq!(report.signed_transactions, 1);
        assert_eq!(report.router_matches, 0);
        assert_eq!(report.recovered_signers, 0);
        assert!(report.candidates.is_empty());
    }

    #[test]
    fn decodes_nested_batch_without_recovering_in_observe_mode() {
        let raw = hex::decode(LEGACY_TX).unwrap();
        let mut signed = vec![L2_MESSAGE_KIND_SIGNED_TX];
        signed.extend_from_slice(&raw);
        let mut batch = vec![L2_MESSAGE_KIND_BATCH];
        batch.extend_from_slice(&(signed.len() as u64).to_be_bytes());
        batch.extend_from_slice(&signed);
        let mut decoder = FeedDecoder::new(Filter::default());
        let report = decoder.decode(&feed_with_l2(&batch)).unwrap();

        assert_eq!(report.signed_transactions, 1);
        assert_eq!(report.recovered_signers, 0);
    }

    #[test]
    fn visitor_observes_target_without_signer_recovery() {
        let raw = hex::decode(LEGACY_TX).unwrap();
        let mut l2 = vec![L2_MESSAGE_KIND_SIGNED_TX];
        l2.extend_from_slice(&raw);
        let mut decoder = FeedDecoder::new(Filter::default());
        let mut observed = Vec::new();
        let report = decoder
            .decode_with(&feed_with_l2(&l2), |context| {
                observed.push((
                    context.sequence_number,
                    context.l1_block_number,
                    context.l1_timestamp,
                    context.transaction.to(),
                    *context.transaction.tx_hash(),
                ));
            })
            .unwrap();

        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].0, 42);
        assert_eq!(observed[0].1, 9);
        assert_eq!(observed[0].2, 10);
        assert_eq!(observed[0].3, Some(Address::ZERO));
        assert_eq!(report.recovered_signers, 0);
        assert!(report.candidates.is_empty());
    }

    #[test]
    fn recovers_signer_only_after_router_match() {
        let raw = hex::decode(LEGACY_TX).unwrap();
        let mut l2 = vec![L2_MESSAGE_KIND_SIGNED_TX];
        l2.extend_from_slice(&raw);
        let mut filter = Filter::default();
        filter.routers.insert(Address::ZERO);
        let mut decoder = FeedDecoder::new(filter);
        let report = decoder.decode(&feed_with_l2(&l2)).unwrap();

        assert_eq!(report.router_matches, 1);
        assert_eq!(report.selector_matches, 1);
        assert_eq!(report.recovered_signers, 1);
        assert_eq!(report.candidates.len(), 1);
        assert_eq!(report.candidates[0].to, Address::ZERO);
    }

    #[test]
    fn filters_selector_before_recovering_signer() {
        let raw = hex::decode(LEGACY_TX).unwrap();
        let mut l2 = vec![L2_MESSAGE_KIND_SIGNED_TX];
        l2.extend_from_slice(&raw);
        let mut filter = Filter::default();
        filter.routers.insert(Address::ZERO);
        filter.selectors.insert([0x12, 0x34, 0x56, 0x78]);
        let mut decoder = FeedDecoder::new(filter);
        let report = decoder.decode(&feed_with_l2(&l2)).unwrap();

        assert_eq!(report.router_matches, 1);
        assert_eq!(report.selector_matches, 0);
        assert_eq!(report.recovered_signers, 0);
        assert!(report.candidates.is_empty());
    }

    #[test]
    fn optionally_emits_transaction_fingerprints() {
        let raw = hex::decode(LEGACY_TX).unwrap();
        let mut l2 = vec![L2_MESSAGE_KIND_SIGNED_TX];
        l2.extend_from_slice(&raw);
        let filter = Filter {
            emit_transaction_hashes: true,
            ..Filter::default()
        };
        let mut decoder = FeedDecoder::new(filter);
        let report = decoder.decode(&feed_with_l2(&l2)).unwrap();

        assert_eq!(report.transaction_fingerprints.len(), 1);
        assert_eq!(report.transaction_fingerprints[0].sequence_number, 42);
    }

    #[test]
    fn rejects_truncated_batch() {
        let feed = feed_with_l2(&[L2_MESSAGE_KIND_BATCH, 0, 1]);
        let mut decoder = FeedDecoder::new(Filter::default());
        assert!(matches!(
            decoder.decode(&feed),
            Err(DecodeError::TruncatedBatchLength { sequence: 42 })
        ));
    }
}
