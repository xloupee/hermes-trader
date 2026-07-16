use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

use alloy_primitives::{Address, B256, keccak256};
use alloy_sol_types::{SolCall, sol};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::clanker_receipt_quote::{
    CLANKER_DESCENDING_MEV_MODULE, CLANKER_EXTENSION, CLANKER_STATIC_HOOK,
};
use hermes_feed::flap_identity::{
    FLAP_PORTAL_IMPLEMENTATION, FLAP_PORTAL_PROXY, FLAP_VAULT_PORTAL_IMPLEMENTATION,
    FLAP_VAULT_PORTAL_PROXY,
};
use hermes_feed::hood_receipt_quote::HoodIdentityRole;
use hermes_feed::launchpad_adapters::{
    CLANKER_FACTORY, CLANKER_LOCKER, DOPPLER_CREATE_EMITTER, V4_POOL_MANAGER,
};
use hermes_feed::paper_observer::{
    ObservedPinsDocumentRole, ObservedPinsProvenance, ObservedRuntimePin, PaperExpectedPins,
    PaperLaunchpadObserver, PaperObservedStartupSnapshot,
};
use hermes_feed::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, BOW_LAUNCH_FACTORY, CHAIN_ID, LAUNCHHOOD_V3_FACTORY,
    NOXA_LAUNCH_FACTORY, PUBLIC_RPC_URL, UNISWAP_V3_FACTORY, UNISWAP_V3_POSITION_MANAGER,
    UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use hermes_feed::smart_account::{
    AccountExecutionProfile, ContractPin, ENTRY_POINT_V07, EntryPointCall, SmartAccountPin,
    SmartAccountPins, decode_entry_point_v07,
};
use hermes_feed::tier2_curve::{
    HOOD_FACTORY, LEAVEHOOD_CORE_IMPLEMENTATION, LEAVEHOOD_CORE_PROXY,
    LEAVEHOOD_FACTORY_IMPLEMENTATION, LEAVEHOOD_FACTORY_PROXY,
};
use hermes_feed::{NoxaRpcClient, PonsAdapter};
use serde::Serialize;

const BANKR_PROOF_TX: B256 =
    alloy_primitives::b256!("c6597fe88f8f3f16b4ba6613c25050d75dc6f3c2b2c5315f0b47828f98f0609c");
const BANKR_PROOF_ACCOUNT: Address =
    alloy_primitives::address!("ff89978cb8171132395741b785d4a1f7e3efa124");
const BANKR_KERNEL_IMPLEMENTATION: Address =
    alloy_primitives::address!("d6cedde84be40893d153be9d467cd6ad37875b28");
const BANKR_ACCOUNT_DESIGNATOR_HASH: B256 =
    alloy_primitives::b256!("4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4");
const BANKR_KERNEL_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d");
const BANKR_ACCOUNT_EXECUTE_SELECTOR: [u8; 4] = [0xe9, 0xae, 0x5c, 0x53];
const BANKR_DOPPLER_CREATE_SELECTOR: [u8; 4] = [0x88, 0x2d, 0xb7, 0x07];

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Read-only chain-4663 runtime-pin snapshot and Bankr proof inspector"
)]
struct Cli {
    #[arg(long, default_value = PUBLIC_RPC_URL)]
    rpc_url: String,

    /// Pin an independently reviewed historical L2 block instead of latest.
    #[arg(long)]
    l2_block: Option<u64>,

    /// Optional reviewed expected pins. When supplied, startup validation must pass.
    #[arg(long)]
    expected_pins: Option<PathBuf>,

    /// Write the exact observer-compatible startup snapshot to this file.
    #[arg(long)]
    snapshot_output: Option<PathBuf>,

    /// Known Bankr/Doppler ERC-4337 proof transaction to inspect.
    #[arg(long, default_value_t = BANKR_PROOF_TX)]
    bankr_proof_tx: B256,
}

#[derive(Debug, Clone, Copy)]
struct PinRequest {
    address: Address,
    implementation: Option<Address>,
}

#[derive(Debug, Serialize)]
struct BankrProof {
    transaction_hash: B256,
    outer_bundler: Address,
    leader: Address,
    entry_point: Address,
    account_selector: String,
    account_designator_hash: B256,
    delegation_implementation: Address,
    delegation_runtime_hash: B256,
    target: Option<Address>,
    target_runtime_hash: Option<B256>,
    selector: Option<String>,
    unwrap_depth: Option<usize>,
    inner_call_count: Option<usize>,
    decode_error: Option<String>,
}

sol! {
    struct DiagnosticPackedUserOperation {
        address sender;
        uint256 nonce;
        bytes initCode;
        bytes callData;
        bytes32 accountGasLimits;
        uint256 preVerificationGas;
        bytes32 gasFees;
        bytes paymasterAndData;
        bytes signature;
    }

    function handleOps(
        DiagnosticPackedUserOperation[] ops,
        address payable beneficiary
    ) external;
}

#[derive(Debug, Serialize)]
struct SnapshotReport {
    record_type: &'static str,
    chain_id: u64,
    pinned_l2_block: u64,
    pin_count: usize,
    expected_validation_passed: bool,
    bankr_proof: BankrProof,
    rpc_metrics: hermes_feed::RpcMetricsSnapshot,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let chain_id = rpc.chain_id().await?;
    if chain_id != CHAIN_ID {
        bail!("RPC chain ID {chain_id} does not match Robinhood {CHAIN_ID}");
    }
    let pinned_l2_block = match args.l2_block {
        Some(block) => block,
        None => rpc.latest_block_number().await?,
    };

    let expected = args
        .expected_pins
        .as_ref()
        .map(|path| {
            serde_json::from_reader(BufReader::new(
                File::open(path)
                    .with_context(|| format!("open expected pins {}", path.display()))?,
            ))
            .with_context(|| format!("decode expected pins {}", path.display()))
        })
        .transpose()?;

    let requests = pin_requests(expected.as_ref())?;
    let mut pins = Vec::with_capacity(requests.len());
    for request in requests {
        let code = rpc
            .code_at_l2_block(request.address, pinned_l2_block)
            .await
            .with_context(|| format!("read runtime code for {}", request.address))?;
        if code.is_empty() {
            bail!("runtime code is empty for {}", request.address);
        }
        pins.push(ObservedRuntimePin {
            address: request.address,
            implementation: request.implementation,
            runtime_hash: keccak256(&code),
            code_bytes: Some(code.len()),
        });
    }
    let hood_protocol =
        if let Some(profile) = expected.as_ref().and_then(|pins| pins.hood_curve.as_ref()) {
            profile.validate()?;
            let address = |role| {
                profile
                    .identity(role)
                    .map(|identity| identity.address)
                    .with_context(|| format!("Hood {role:?} identity missing"))
            };
            Some(
                rpc.hood_protocol_snapshot_at(
                    address(HoodIdentityRole::Factory)?,
                    address(HoodIdentityRole::Migrator)?,
                    address(HoodIdentityRole::Locker)?,
                    address(HoodIdentityRole::PositionManager)?,
                    address(HoodIdentityRole::SwapRouter)?,
                    address(HoodIdentityRole::OwnerSafeProxy)?,
                    pinned_l2_block,
                )
                .await?,
            )
        } else {
            None
        };
    let snapshot = PaperObservedStartupSnapshot {
        schema_version: 3,
        document_role: ObservedPinsDocumentRole::ObservedStartupSnapshot,
        provenance: ObservedPinsProvenance::StartupObservation,
        fixture_id: None,
        chain_id,
        pins,
        hood_protocol,
    };

    let proof = inspect_bankr_proof(&rpc, pinned_l2_block, args.bankr_proof_tx).await?;
    let expected_validation_passed = if let Some(expected) = expected {
        PaperLaunchpadObserver::from_startup_snapshots(expected, snapshot.clone())?;
        true
    } else {
        false
    };

    if let Some(path) = args.snapshot_output {
        serde_json::to_writer_pretty(
            BufWriter::new(
                File::create(&path)
                    .with_context(|| format!("create snapshot {}", path.display()))?,
            ),
            &snapshot,
        )
        .with_context(|| format!("write snapshot {}", path.display()))?;
    }

    println!(
        "{}",
        serde_json::to_string(&SnapshotReport {
            record_type: "launchpad_pin_snapshot",
            chain_id,
            pinned_l2_block,
            pin_count: snapshot.pins.len(),
            expected_validation_passed,
            bankr_proof: proof,
            rpc_metrics: rpc.metrics(),
        })?
    );
    Ok(())
}

fn pin_requests(expected: Option<&PaperExpectedPins>) -> Result<Vec<PinRequest>> {
    let mut requests = vec![
        request(WETH, None),
        request(UNISWAP_V3_FACTORY, None),
        request(UNISWAP_V3_POSITION_MANAGER, None),
        request(UNISWAP_V3_SWAP_ROUTER_02, None),
        request(NOXA_LAUNCH_FACTORY, None),
        request(ACTIVE_NOXA_LAUNCH_FACTORY, None),
        request(BOW_LAUNCH_FACTORY, None),
        request(LAUNCHHOOD_V3_FACTORY, None),
        request(CLANKER_FACTORY, None),
        request(DOPPLER_CREATE_EMITTER, None),
        request(V4_POOL_MANAGER, None),
        request(ENTRY_POINT_V07, None),
        request(BANKR_PROOF_ACCOUNT, Some(BANKR_KERNEL_IMPLEMENTATION)),
        request(BANKR_KERNEL_IMPLEMENTATION, None),
        request(HOOD_FACTORY, None),
        request(FLAP_PORTAL_PROXY, Some(FLAP_PORTAL_IMPLEMENTATION)),
        request(FLAP_PORTAL_IMPLEMENTATION, None),
        request(
            FLAP_VAULT_PORTAL_PROXY,
            Some(FLAP_VAULT_PORTAL_IMPLEMENTATION),
        ),
        request(FLAP_VAULT_PORTAL_IMPLEMENTATION, None),
    ];
    requests.extend(
        PonsAdapter::required_startup_identities()
            .iter()
            .map(|identity| request(identity.address, None)),
    );

    if let Some(expected) = expected {
        if let Some(profile) = &expected.hood_curve {
            profile.validate()?;
            let singleton = profile
                .identity(HoodIdentityRole::OwnerSafeSingleton)
                .context("Hood Safe singleton identity missing")?;
            requests.extend(profile.identities.iter().map(|identity| {
                request(
                    identity.address,
                    (identity.role == HoodIdentityRole::OwnerSafeProxy)
                        .then_some(singleton.address),
                )
            }));
        }
        if let Some(configured) = expected.clanker_v4 {
            configured.expected_profile()?;
            requests.extend([
                request(CLANKER_STATIC_HOOK, None),
                request(CLANKER_LOCKER, None),
                request(CLANKER_DESCENDING_MEV_MODULE, None),
                request(CLANKER_EXTENSION, None),
            ]);
        }
        if let Some(configured) = expected.bankr_doppler_v4 {
            let profile = configured.expected_profile()?;
            requests.extend([
                request(profile.airlock.address, None),
                request(profile.pool_manager.address, None),
                request(profile.initializer.address, None),
                request(profile.rehype_hook.address, None),
                request(profile.token_factory.address, None),
                request(profile.governance_factory.address, None),
                request(profile.liquidity_migrator.address, None),
                request(profile.weth.address, None),
                request(profile.entry_point.address, None),
                request(
                    profile.smart_account.account.address,
                    profile
                        .smart_account
                        .delegation_implementation
                        .map(|pin| pin.address),
                ),
            ]);
            if let Some(implementation) = profile.smart_account.delegation_implementation {
                requests.push(request(implementation.address, None));
            }
        }
        if expected.leavehood_factory_proxy_runtime_hash.is_some() {
            requests.extend([
                request(
                    LEAVEHOOD_FACTORY_PROXY,
                    Some(LEAVEHOOD_FACTORY_IMPLEMENTATION),
                ),
                request(LEAVEHOOD_FACTORY_IMPLEMENTATION, None),
            ]);
        }
        if expected.leavehood_core_proxy_runtime_hash.is_some() {
            requests.extend([
                request(LEAVEHOOD_CORE_PROXY, Some(LEAVEHOOD_CORE_IMPLEMENTATION)),
                request(LEAVEHOOD_CORE_IMPLEMENTATION, None),
            ]);
        }
        requests.extend(
            expected
                .bankr_doppler_calls
                .iter()
                .map(|call| request(call.destination, None)),
        );
        if let Some(smart) = &expected.erc4337 {
            requests.push(request(ENTRY_POINT_V07, None));
            for account in &smart.accounts {
                requests.push(request(account.account, account.delegation_implementation));
                if let Some(factory) = account.factory {
                    requests.push(request(factory, None));
                }
                if let Some(implementation) = account.delegation_implementation {
                    requests.push(request(implementation, None));
                }
            }
        }
    }

    let mut seen = HashSet::new();
    requests.retain(|pin| seen.insert(pin.address));
    Ok(requests)
}

const fn request(address: Address, implementation: Option<Address>) -> PinRequest {
    PinRequest {
        address,
        implementation,
    }
}

async fn inspect_bankr_proof(
    rpc: &NoxaRpcClient,
    pinned_l2_block: u64,
    transaction_hash: B256,
) -> Result<BankrProof> {
    let transaction = rpc
        .transaction_by_hash(transaction_hash)
        .await?
        .with_context(|| format!("Bankr proof transaction {transaction_hash} is missing"))?;
    if transaction.to != Some(ENTRY_POINT_V07) {
        bail!("Bankr proof transaction no longer targets the canonical EntryPoint");
    }
    let diagnostic = handleOpsCall::abi_decode(&transaction.input)
        .context("decode Bankr proof handleOps envelope")?;
    if diagnostic.ops.len() != 1 || diagnostic.ops[0].sender != BANKR_PROOF_ACCOUNT {
        bail!("Bankr proof contains an unexpected operation set");
    }
    let account_selector = diagnostic.ops[0]
        .callData
        .get(..4)
        .context("Bankr proof account calldata has no selector")?;
    if account_selector != BANKR_ACCOUNT_EXECUTE_SELECTOR {
        bail!("Bankr proof account selector drifted from ERC-7579 execute(bytes32,bytes)");
    }
    let entry_point_code = rpc
        .code_at_l2_block(ENTRY_POINT_V07, pinned_l2_block)
        .await?;
    let account_code = rpc
        .code_at_l2_block(BANKR_PROOF_ACCOUNT, pinned_l2_block)
        .await?;
    let kernel_code = rpc
        .code_at_l2_block(BANKR_KERNEL_IMPLEMENTATION, pinned_l2_block)
        .await?;
    let target_code = rpc
        .code_at_l2_block(DOPPLER_CREATE_EMITTER, pinned_l2_block)
        .await?;
    if entry_point_code.is_empty()
        || account_code.is_empty()
        || kernel_code.is_empty()
        || target_code.is_empty()
    {
        bail!("Bankr proof identity has empty runtime code");
    }
    if keccak256(&account_code) != BANKR_ACCOUNT_DESIGNATOR_HASH
        || account_code.as_ref()
            != [
                vec![0xef, 0x01, 0x00],
                BANKR_KERNEL_IMPLEMENTATION.as_slice().to_vec(),
            ]
            .concat()
        || keccak256(&kernel_code) != BANKR_KERNEL_RUNTIME_HASH
    {
        bail!("Bankr proof EIP-7702 designator or delegated Kernel runtime drifted");
    }
    let entry_point = ContractPin {
        address: ENTRY_POINT_V07,
        runtime_code_hash: keccak256(&entry_point_code),
    };
    let account = SmartAccountPin {
        account: ContractPin {
            address: BANKR_PROOF_ACCOUNT,
            runtime_code_hash: BANKR_ACCOUNT_DESIGNATOR_HASH,
        },
        execution_profile: AccountExecutionProfile::Erc7579SingleCall,
        factory: None,
        delegation_implementation: Some(ContractPin {
            address: BANKR_KERNEL_IMPLEMENTATION,
            runtime_code_hash: BANKR_KERNEL_RUNTIME_HASH,
        }),
    };
    let target = ContractPin {
        address: DOPPLER_CREATE_EMITTER,
        runtime_code_hash: keccak256(&target_code),
    };
    let decoded = decode_entry_point_v07(
        EntryPointCall {
            chain_id: CHAIN_ID,
            destination: entry_point,
            outer_bundler: transaction.from,
            calldata: &transaction.input,
        },
        SmartAccountPins {
            entry_point,
            accounts: std::slice::from_ref(&account),
            allowed_targets: std::slice::from_ref(&target),
        },
    );
    match decoded {
        Ok(decoded) => {
            if decoded.leader != BANKR_PROOF_ACCOUNT || decoded.target != DOPPLER_CREATE_EMITTER {
                bail!("Bankr proof decoded an unexpected leader or target");
            }
            let selector = decoded
                .calldata
                .get(..4)
                .context("Bankr proof inner calldata has no selector")?;
            if decoded.value != alloy_primitives::U256::ZERO
                || selector != BANKR_DOPPLER_CREATE_SELECTOR
            {
                bail!("Bankr proof inner value or Doppler selector drifted");
            }
            Ok(BankrProof {
                transaction_hash,
                outer_bundler: decoded.outer_bundler,
                leader: decoded.leader,
                entry_point: decoded.entry_point,
                account_selector: format!("0x{}", hex::encode(account_selector)),
                account_designator_hash: BANKR_ACCOUNT_DESIGNATOR_HASH,
                delegation_implementation: BANKR_KERNEL_IMPLEMENTATION,
                delegation_runtime_hash: BANKR_KERNEL_RUNTIME_HASH,
                target: Some(decoded.target),
                target_runtime_hash: Some(target.runtime_code_hash),
                selector: Some(format!("0x{}", hex::encode(selector))),
                unwrap_depth: Some(decoded.unwrap_depth),
                inner_call_count: Some(decoded.inner_call_count),
                decode_error: None,
            })
        }
        Err(error) => Ok(BankrProof {
            transaction_hash,
            outer_bundler: transaction.from,
            leader: BANKR_PROOF_ACCOUNT,
            entry_point: ENTRY_POINT_V07,
            account_selector: format!("0x{}", hex::encode(account_selector)),
            account_designator_hash: BANKR_ACCOUNT_DESIGNATOR_HASH,
            delegation_implementation: BANKR_KERNEL_IMPLEMENTATION,
            delegation_runtime_hash: BANKR_KERNEL_RUNTIME_HASH,
            target: None,
            target_runtime_hash: None,
            selector: None,
            unwrap_depth: None,
            inner_call_count: None,
            decode_error: Some(error.to_string()),
        }),
    }
}

#[cfg(test)]
mod tests {
    use hermes_feed::BankrDopplerExpectedProfile;

    use super::*;

    #[test]
    fn production_bankr_snapshot_requests_every_reviewed_dependency() {
        let expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        let profile = BankrDopplerExpectedProfile::production();
        let requests = pin_requests(Some(&expected)).unwrap();
        for address in [
            profile.airlock.address,
            profile.pool_manager.address,
            profile.initializer.address,
            profile.rehype_hook.address,
            profile.token_factory.address,
            profile.governance_factory.address,
            profile.liquidity_migrator.address,
            profile.weth.address,
            profile.entry_point.address,
            profile.smart_account.account.address,
            profile
                .smart_account
                .delegation_implementation
                .unwrap()
                .address,
        ] {
            assert_eq!(
                requests
                    .iter()
                    .filter(|request| request.address == address)
                    .count(),
                1,
                "missing or duplicate request for {address}"
            );
        }
        assert_eq!(
            requests
                .iter()
                .find(|request| request.address == profile.smart_account.account.address)
                .unwrap()
                .implementation,
            profile
                .smart_account
                .delegation_implementation
                .map(|pin| pin.address)
        );
    }
}
