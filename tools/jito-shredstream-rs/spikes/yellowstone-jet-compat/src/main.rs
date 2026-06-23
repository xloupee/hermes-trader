use solana_keypair::Keypair;
use solana_message::{v0, VersionedMessage};
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_system_interface::instruction::transfer;
use solana_transaction::versioned::VersionedTransaction;
use yellowstone_jet_tpu_client::{
    core::TpuSenderResponse,
    yellowstone_grpc::sender::{create_yellowstone_tpu_sender, Endpoints, NewYellowstoneTpuSender},
};

#[tokio::main]
async fn main() {
    let identity = Keypair::new();
    let recipient = Pubkey::new_unique();
    let blockhash = solana_hash::Hash::new_unique();
    let instruction = transfer(&identity.pubkey(), &recipient, 1);

    let transaction = VersionedTransaction::try_new(
        VersionedMessage::V0(
            v0::Message::try_compile(&identity.pubkey(), &[instruction], &[], blockhash)
                .expect("compile v0 transfer"),
        ),
        &[&identity],
    )
    .expect("sign versioned transaction");

    let signature = transaction.signatures[0];
    let wire_transaction = bincode::serialize(&transaction).expect("serialize transaction");

    // Keep the real constructor typechecked without connecting in this compile-only spike.
    let _constructor = create_yellowstone_tpu_sender;
    let _endpoints = Endpoints {
        rpc: String::new(),
        grpc: String::new(),
        grpc_x_token: None,
    };
    let _maybe_sender: Option<NewYellowstoneTpuSender> = None;
    let _maybe_response: Option<TpuSenderResponse> = None;

    // This is the exact shape the production lane needs: known signature plus
    // already-serialized transaction bytes. A real lane calls sender.send_txn().
    println!(
        "jet-compatible-wire-transaction signature={} bytes={}",
        signature,
        wire_transaction.len()
    );
}
