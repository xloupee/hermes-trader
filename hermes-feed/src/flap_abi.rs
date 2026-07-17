use alloy_primitives::{Address, B256, U256};
use alloy_sol_types::{SolEvent, sol};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::flap_identity::{FLAP_PORTAL_PROXY, FLAP_VAULT_PORTAL_PROXY, FlapPortalVariant};
use crate::launchpad_adapter::{
    ActionKind, LaunchpadId, MarketIdentity, ObservedAmounts, ObservedLeaderAction, ObservedRoute,
};
use crate::noxa_abi::ReceiptLog;
use crate::robinhood::CHAIN_ID;

sol! {
    event TokenCreated(
        uint256 id,
        address creator,
        uint256 nonce,
        address token,
        string name,
        string symbol,
        string metadata
    );

    event TokenBought(
        uint256 id,
        address token,
        address buyer,
        uint256 amount,
        uint256 quoteAmount,
        uint256 fee,
        uint256 circulatingSupply
    );

    event TokenSold(
        uint256 id,
        address token,
        address seller,
        uint256 amount,
        uint256 quoteAmount,
        uint256 fee,
        uint256 circulatingSupply
    );
}

pub const FLAP_TOKEN_CREATED_TOPIC: B256 = TokenCreated::SIGNATURE_HASH;
pub const FLAP_TOKEN_BOUGHT_TOPIC: B256 = TokenBought::SIGNATURE_HASH;
pub const FLAP_TOKEN_SOLD_TOPIC: B256 = TokenSold::SIGNATURE_HASH;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FlapTokenCreated {
    pub source: FlapPortalVariant,
    pub id: U256,
    pub creator: Address,
    pub nonce: U256,
    pub token: Address,
    pub name: String,
    pub symbol: String,
    pub metadata: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FlapTokenBought {
    pub source: FlapPortalVariant,
    pub id: U256,
    pub token: Address,
    pub buyer: Address,
    pub amount: U256,
    /// Raw ABI field only. It is deliberately not normalized or labeled ETH;
    /// the quote token and decimals must come from the pinned token profile.
    pub raw_quote_amount: U256,
    pub raw_fee: U256,
    pub circulating_supply: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FlapNormalizationError {
    #[error("verified market does not match the observed Flap token")]
    MarketMismatch,
    #[error("normalized quote amount is unavailable")]
    QuoteNormalizationUnavailable,
}

impl FlapTokenBought {
    /// Normalize only after an asynchronously verified token profile has
    /// supplied the quote asset, pool identity, and quote decimals. Raw event
    /// fields are never relabeled as ETH in the candidate path.
    pub fn normalize_observed_buy(
        &self,
        tx_hash: B256,
        market: MarketIdentity,
        normalized_quote_amount: U256,
    ) -> Result<ObservedLeaderAction, FlapNormalizationError> {
        if market.token != self.token
            || market.quote_asset == Address::ZERO
            || market.pool == Address::ZERO
        {
            return Err(FlapNormalizationError::MarketMismatch);
        }
        if normalized_quote_amount == U256::ZERO {
            return Err(FlapNormalizationError::QuoteNormalizationUnavailable);
        }
        Ok(ObservedLeaderAction {
            tx_hash,
            launchpad: LaunchpadId::Flap,
            leader: self.buyer,
            action: ActionKind::Buy,
            market,
            asset_in: market.quote_asset,
            asset_out: market.token,
            observed_amounts: ObservedAmounts {
                amount_in: normalized_quote_amount,
                minimum_out: U256::ZERO,
            },
            observed_route: ObservedRoute::FlapPortal,
        })
    }
}

fn source_variant(address: Address) -> Option<FlapPortalVariant> {
    match address {
        FLAP_PORTAL_PROXY => Some(FlapPortalVariant::Portal),
        FLAP_VAULT_PORTAL_PROXY => Some(FlapPortalVariant::VaultPortal),
        _ => None,
    }
}

/// Strict receipt observation. The explicit chain argument prevents replaying
/// a same-address/same-topic fixture from Base or another EVM network.
pub fn decode_flap_token_created(chain_id: u64, log: &ReceiptLog) -> Option<FlapTokenCreated> {
    if chain_id != CHAIN_ID {
        return None;
    }
    // TokenCreated is canonical launch evidence only when emitted by Portal.
    // VaultPortal is a caller in the reviewed vault route and must never be
    // allowed to self-attest a launch with a lookalike event.
    if log.address != FLAP_PORTAL_PROXY {
        return None;
    }
    let decoded =
        TokenCreated::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()?;
    if decoded.encode_data().as_slice() != log.data.as_ref() {
        return None;
    }
    if decoded.creator == Address::ZERO || decoded.token == Address::ZERO {
        return None;
    }
    Some(FlapTokenCreated {
        source: FlapPortalVariant::Portal,
        id: decoded.id,
        creator: decoded.creator,
        nonce: decoded.nonce,
        token: decoded.token,
        name: decoded.name,
        symbol: decoded.symbol,
        metadata: decoded.metadata,
    })
}

pub fn decode_flap_token_bought(chain_id: u64, log: &ReceiptLog) -> Option<FlapTokenBought> {
    if chain_id != CHAIN_ID {
        return None;
    }
    let source = source_variant(log.address)?;
    let decoded =
        TokenBought::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()?;
    if decoded.encode_data().as_slice() != log.data.as_ref() {
        return None;
    }
    if decoded.token == Address::ZERO
        || decoded.buyer == Address::ZERO
        || decoded.amount == U256::ZERO
    {
        return None;
    }
    Some(FlapTokenBought {
        source,
        id: decoded.id,
        token: decoded.token,
        buyer: decoded.buyer,
        amount: decoded.amount,
        raw_quote_amount: decoded.quoteAmount,
        raw_fee: decoded.fee,
        circulating_supply: decoded.circulatingSupply,
    })
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Bytes, address, b256};

    use super::*;

    fn created_log(address: Address) -> ReceiptLog {
        let event = TokenCreated {
            id: U256::from(17),
            creator: address!("1111111111111111111111111111111111111111"),
            nonce: U256::from(3),
            token: address!("2222222222222222222222222222222222222222"),
            name: "Robinhood Flap".into(),
            symbol: "FLAP".into(),
            metadata: "ipfs://fixture".into(),
        };
        ReceiptLog {
            address,
            log_index: 7,
            topics: vec![TokenCreated::SIGNATURE_HASH],
            data: Bytes::from(event.encode_data()),
        }
    }

    fn bought_log(address: Address) -> ReceiptLog {
        let event = TokenBought {
            id: U256::from(17),
            token: address!("2222222222222222222222222222222222222222"),
            buyer: address!("3333333333333333333333333333333333333333"),
            amount: U256::from(100),
            quoteAmount: U256::from(10),
            fee: U256::from(1),
            circulatingSupply: U256::from(1_000),
        };
        ReceiptLog {
            address,
            log_index: 8,
            topics: vec![TokenBought::SIGNATURE_HASH],
            data: Bytes::from(event.encode_data()),
        }
    }

    #[test]
    fn observes_only_portal_token_created_and_keeps_vault_buys_separate() {
        let portal = decode_flap_token_created(CHAIN_ID, &created_log(FLAP_PORTAL_PROXY)).unwrap();
        assert_eq!(portal.source, FlapPortalVariant::Portal);
        assert_eq!(portal.symbol, "FLAP");

        assert!(
            decode_flap_token_created(CHAIN_ID, &created_log(FLAP_VAULT_PORTAL_PROXY)).is_none()
        );

        let buy = decode_flap_token_bought(CHAIN_ID, &bought_log(FLAP_PORTAL_PROXY)).unwrap();
        assert_eq!(buy.raw_quote_amount, U256::from(10));
        let normalized = buy
            .normalize_observed_buy(
                B256::with_last_byte(0x44),
                MarketIdentity {
                    token: buy.token,
                    quote_asset: Address::with_last_byte(0xaa),
                    pool: Address::with_last_byte(0xbb),
                },
                U256::from(10),
            )
            .unwrap();
        assert_eq!(normalized.launchpad, LaunchpadId::Flap);
        assert_eq!(normalized.action, ActionKind::Buy);
    }

    #[test]
    fn rejects_chain_mismatch_and_cross_adapter_address() {
        assert!(decode_flap_token_created(8_453, &created_log(FLAP_PORTAL_PROXY)).is_none());
        assert!(
            decode_flap_token_created(
                CHAIN_ID,
                &created_log(address!("d9ec2db5f3d1b236843925949fe5bd8a3836fccb"))
            )
            .is_none()
        );
    }

    #[test]
    fn rejects_malformed_topics_data_and_zero_principals() {
        let mut malformed = created_log(FLAP_PORTAL_PROXY);
        malformed.topics[0] =
            b256!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(decode_flap_token_created(CHAIN_ID, &malformed).is_none());

        let mut malformed = created_log(FLAP_PORTAL_PROXY);
        let truncated_len = malformed.data.len() - 1;
        malformed.data.truncate(truncated_len);
        assert!(decode_flap_token_created(CHAIN_ID, &malformed).is_none());

        let zero = TokenCreated {
            id: U256::ZERO,
            creator: Address::ZERO,
            nonce: U256::ZERO,
            token: Address::ZERO,
            name: String::new(),
            symbol: String::new(),
            metadata: String::new(),
        };
        let zero = ReceiptLog {
            address: FLAP_PORTAL_PROXY,
            log_index: 0,
            topics: vec![TokenCreated::SIGNATURE_HASH],
            data: Bytes::from(zero.encode_data()),
        };
        assert!(decode_flap_token_created(CHAIN_ID, &zero).is_none());
    }
}
