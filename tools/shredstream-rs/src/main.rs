use anyhow::{Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use serde::Serialize;
use solana_entry::entry::Entry as SolanaEntry;
use std::time::{SystemTime, UNIX_EPOCH};
use tonic::transport::Channel;

#[derive(Debug, Parser)]
#[command(
    name = "shredstream-rs",
    about = "Jito ShredStream gRPC deshred decoder that emits normalized transaction JSONL"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Watch {
        #[arg(long, env = "SHREDSTREAM_GRPC_URL")]
        grpc_url: String,
    },
    DecodeBase64 {
        #[arg(long)]
        slot: u64,
        #[arg(long)]
        entries_base64: String,
        #[arg(long)]
        received_at_ms: Option<u64>,
    },
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct SubscribeEntriesRequest {}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ShredstreamEntry {
    #[prost(uint64, tag = "1")]
    pub slot: u64,
    #[prost(bytes = "vec", tag = "2")]
    pub entries: Vec<u8>,
}

pub mod shredstream_proxy_client {
    use super::{ShredstreamEntry, SubscribeEntriesRequest};
    use tonic::codegen::*;

    #[derive(Debug, Clone)]
    pub struct ShredstreamProxyClient<T> {
        inner: tonic::client::Grpc<T>,
    }

    impl ShredstreamProxyClient<tonic::transport::Channel> {
        pub async fn connect<D>(dst: D) -> Result<Self, tonic::transport::Error>
        where
            D: TryInto<tonic::transport::Endpoint>,
            D::Error: Into<StdError>,
        {
            let conn = tonic::transport::Endpoint::new(dst)?.connect().await?;
            Ok(Self::new(conn))
        }
    }

    impl<T> ShredstreamProxyClient<T>
    where
        T: tonic::client::GrpcService<tonic::body::Body>,
        T::Error: Into<StdError>,
        T::ResponseBody: Body<Data = Bytes> + Send + 'static,
        <T::ResponseBody as Body>::Error: Into<StdError> + Send,
    {
        pub fn new(inner: T) -> Self {
            Self {
                inner: tonic::client::Grpc::new(inner),
            }
        }

        pub async fn subscribe_entries(
            &mut self,
            request: impl tonic::IntoRequest<SubscribeEntriesRequest>,
        ) -> Result<tonic::Response<tonic::codec::Streaming<ShredstreamEntry>>, tonic::Status> {
            self.inner.ready().await.map_err(|error| {
                tonic::Status::unknown(format!("service was not ready: {}", error.into()))
            })?;
            let codec = tonic::codec::ProstCodec::default();
            let path =
                http::uri::PathAndQuery::from_static("/shredstream.ShredstreamProxy/SubscribeEntries");
            self.inner
                .server_streaming(request.into_request(), path, codec)
                .await
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalizedTransaction {
    slot: u64,
    signature: String,
    received_at_ms: u64,
    account_keys: Vec<String>,
    address_table_lookups: Vec<NormalizedAddressTableLookup>,
    instructions: Vec<NormalizedInstruction>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalizedAddressTableLookup {
    account_key: String,
    writable_indexes: Vec<u8>,
    readonly_indexes: Vec<u8>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NormalizedInstruction {
    program_id_index: u8,
    program_id: Option<String>,
    accounts: Vec<u8>,
    data_base64: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Watch { grpc_url } => watch(&grpc_url).await,
        Command::DecodeBase64 {
            slot,
            entries_base64,
            received_at_ms,
        } => {
            let entries = base64::engine::general_purpose::STANDARD
                .decode(entries_base64)
                .context("entries-base64 is not valid base64")?;
            emit_entries(slot, &entries, received_at_ms.unwrap_or_else(now_ms))
        }
    }
}

async fn watch(grpc_url: &str) -> Result<()> {
    let mut client = shredstream_proxy_client::ShredstreamProxyClient::<Channel>::connect(
        normalize_grpc_url(grpc_url),
    )
    .await
    .with_context(|| format!("could not connect to ShredStream gRPC at {grpc_url}"))?;
    let mut stream = client
        .subscribe_entries(SubscribeEntriesRequest {})
        .await
        .context("SubscribeEntries request failed")?
        .into_inner();

    while let Some(slot_entry) = stream.message().await.context("ShredStream receive failed")? {
        emit_entries(slot_entry.slot, &slot_entry.entries, now_ms())?;
    }

    Ok(())
}

fn normalize_grpc_url(value: &str) -> String {
    if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("http://{value}")
    }
}

fn emit_entries(slot: u64, entries_bytes: &[u8], received_at_ms: u64) -> Result<()> {
    for output in decode_entries(slot, entries_bytes, received_at_ms)? {
        println!("{}", serde_json::to_string(&output)?);
    }

    Ok(())
}

fn decode_entries(
    slot: u64,
    entries_bytes: &[u8],
    received_at_ms: u64,
) -> Result<Vec<NormalizedTransaction>> {
    let entries = bincode::deserialize::<Vec<SolanaEntry>>(entries_bytes)
        .context("could not bincode-deserialize Vec<solana_entry::entry::Entry>")?;
    let mut transactions = Vec::new();

    for entry in entries {
        for transaction in entry.transactions {
            let Some(signature) = transaction.signatures.first() else {
                continue;
            };
            let account_keys = transaction
                .message
                .static_account_keys()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let address_table_lookups = address_table_lookups(&transaction.message);
            let instructions = transaction
                .message
                .instructions()
                .iter()
                .map(|instruction| {
                    let program_id = account_keys
                        .get(instruction.program_id_index as usize)
                        .cloned();

                    NormalizedInstruction {
                        program_id_index: instruction.program_id_index,
                        program_id,
                        accounts: instruction.accounts.clone(),
                        data_base64: base64::engine::general_purpose::STANDARD
                            .encode(&instruction.data),
                    }
                })
                .collect::<Vec<_>>();

            let output = NormalizedTransaction {
                slot,
                signature: signature.to_string(),
                received_at_ms,
                account_keys,
                address_table_lookups,
                instructions,
            };

            transactions.push(output);
        }
    }

    Ok(transactions)
}

fn address_table_lookups(
    message: &solana_message::VersionedMessage,
) -> Vec<NormalizedAddressTableLookup> {
    match message {
        solana_message::VersionedMessage::V0(message) => message
            .address_table_lookups
            .iter()
            .map(|lookup| NormalizedAddressTableLookup {
                account_key: lookup.account_key.to_string(),
                writable_indexes: lookup.writable_indexes.clone(),
                readonly_indexes: lookup.readonly_indexes.clone(),
            })
            .collect(),
        solana_message::VersionedMessage::Legacy(_) => Vec::new(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::decode_entries;
    use solana_entry::entry::Entry;
    use solana_hash::Hash;
    use solana_message::{
        compiled_instruction::CompiledInstruction, legacy::Message, MessageHeader,
        VersionedMessage,
    };
    use solana_pubkey::Pubkey;
    use solana_signature::Signature;
    use solana_transaction::versioned::VersionedTransaction;

    #[test]
    fn decodes_bincode_entries_to_normalized_transactions() {
        let program = Pubkey::new_unique();
        let account = Pubkey::new_unique();
        let signature = Signature::from([7; 64]);
        let transaction = VersionedTransaction {
            signatures: vec![signature],
            message: VersionedMessage::Legacy(Message {
                header: MessageHeader {
                    num_required_signatures: 1,
                    num_readonly_signed_accounts: 0,
                    num_readonly_unsigned_accounts: 1,
                },
                account_keys: vec![account, program],
                recent_blockhash: Hash::new_unique(),
                instructions: vec![CompiledInstruction::new_from_raw_parts(
                    1,
                    vec![1, 2, 3, 4],
                    vec![0],
                )],
            }),
        };
        let entries = vec![Entry {
            num_hashes: 1,
            hash: Hash::new_unique(),
            transactions: vec![transaction],
        }];
        let bytes = bincode::serialize(&entries).unwrap();
        let decoded = decode_entries(123, &bytes, 456).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].slot, 123);
        assert_eq!(decoded[0].signature, signature.to_string());
        assert_eq!(decoded[0].received_at_ms, 456);
        assert_eq!(
            decoded[0].account_keys,
            vec![account.to_string(), program.to_string()]
        );
        assert!(decoded[0].address_table_lookups.is_empty());
        assert_eq!(decoded[0].instructions[0].program_id_index, 1);
        assert_eq!(decoded[0].instructions[0].program_id, Some(program.to_string()));
        assert_eq!(decoded[0].instructions[0].accounts, vec![0]);
        assert_eq!(decoded[0].instructions[0].data_base64, "AQIDBA==");
    }
}
