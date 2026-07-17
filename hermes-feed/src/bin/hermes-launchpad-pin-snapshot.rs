use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use alloy_primitives::{Address, B256, Bytes, keccak256};
use alloy_sol_types::{SolCall, SolEvent, sol};
use anyhow::{Context, Result, bail};
use clap::Parser;
use hermes_feed::bankr_receipt_quote::{BANKR_AIRLOCK_RUNTIME_HASH, ENTRY_POINT_V07_RUNTIME_HASH};
use hermes_feed::clanker_receipt_quote::{
    CLANKER_DESCENDING_MEV_MODULE, CLANKER_EXTENSION, CLANKER_STATIC_HOOK,
};
use hermes_feed::flap_identity::{
    FLAP_PORTAL_IMPLEMENTATION, FLAP_PORTAL_PROXY, FLAP_VAULT_PORTAL_IMPLEMENTATION,
    FLAP_VAULT_PORTAL_PROXY,
};
use hermes_feed::hood_receipt_quote::HoodIdentityRole;
use hermes_feed::launchpad_adapters::{
    CLANKER_DEPLOYER, CLANKER_FACTORY, CLANKER_LOCKER, DOPPLER_CREATE_EMITTER, KLIK_FACTORY,
    TRENCH_IMPLEMENTATION, TRENCH_PROXY, V4_POOL_MANAGER,
};
use hermes_feed::paper_observer::{
    ObservedPinsDocumentRole, ObservedPinsProvenance, ObservedRuntimePin, PaperExpectedPins,
    PaperLaunchpadObserver, PaperObservedStartupSnapshot, PinBlockBoundary,
};
use hermes_feed::robinhood::{
    ACTIVE_NOXA_LAUNCH_FACTORY, BOW_LAUNCH_FACTORY, CHAIN_ID, LAUNCHHOOD_V3_FACTORY,
    LAUNCHHOOD_V3_FACTORY_RUNTIME_KECCAK256, LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION,
    LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_CODE_BYTES,
    LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_RUNTIME_KECCAK256, NOXA_LAUNCH_FACTORY, PUBLIC_RPC_URL,
    UNISWAP_V3_FACTORY, UNISWAP_V3_POSITION_MANAGER, UNISWAP_V3_SWAP_ROUTER_02, WETH,
};
use hermes_feed::smart_account::{
    AccountExecutionProfile, ContractPin, ENTRY_POINT_V07, EntryPointCall, SmartAccountPin,
    SmartAccountPins, decode_entry_point_v07,
};
use hermes_feed::tier2_curve::{
    HOOD_FACTORY, LEAVEHOOD_CORE_IMPLEMENTATION, LEAVEHOOD_CORE_PROXY,
    LEAVEHOOD_FACTORY_IMPLEMENTATION, LEAVEHOOD_FACTORY_PROXY,
};
use hermes_feed::{
    Eip7702SelfBatchExpectedPins, NoxaRpcClient, PonsAdapter, PonsPredictionSemantics,
};
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
const BANKR_PROOF_L2_BLOCK: u64 = 10_976_731;
const BANKR_PROOF_BLOCK_HASH: B256 =
    alloy_primitives::b256!("0f1eb4d67209c6d9e30967bf334fcaaecd23e63e694ba9d29d296685830e1529");
const BANKR_PROOF_TRANSACTION_INDEX: u64 = 1;
const EIP1967_IMPLEMENTATION_SLOT: B256 =
    alloy_primitives::b256!("360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc");

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

    /// Use and verify the full historical boundary embedded in expected pins.
    #[arg(long, requires = "expected_pins", conflicts_with = "l2_block")]
    verify_reviewed_boundary: bool,

    /// Latest snapshots use a canonical head this many L2 blocks behind latest.
    #[arg(long, default_value_t = 2)]
    confirmations: u64,

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PinRequest {
    address: Address,
    expected_implementation: Option<Address>,
    resolver: ImplementationResolver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImplementationResolver {
    Direct,
    Eip1967,
    SafeSlot0,
    Eip7702,
}

#[derive(Debug, Serialize)]
struct BankrProof {
    transaction_hash: B256,
    l2_block_number: u64,
    block_hash: B256,
    transaction_index: u64,
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

    event UserOperationEvent(
        bytes32 indexed userOpHash,
        address indexed sender,
        address indexed paymaster,
        uint256 nonce,
        bool success,
        uint256 actualGasCost,
        uint256 actualGasUsed
    );
}

#[derive(Debug, Serialize)]
struct SnapshotReport {
    record_type: &'static str,
    chain_id: u64,
    observed_at: PinBlockBoundary,
    pin_count: usize,
    verified_boundaries: Vec<VerifiedPinBoundary>,
    expected_validation_passed: bool,
    bankr_proof: BankrProof,
    rpc_metrics: hermes_feed::RpcMetricsSnapshot,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
struct VerifiedPinBoundary {
    profile: &'static str,
    observed_at: PinBlockBoundary,
    pin_count: usize,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Cli::parse();
    if args.l2_block.is_none() && !args.verify_reviewed_boundary && args.confirmations == 0 {
        bail!("latest startup snapshots require at least one confirmation");
    }
    let rpc = NoxaRpcClient::with_url(args.rpc_url)?;
    let chain_id = rpc.chain_id().await?;
    if chain_id != CHAIN_ID {
        bail!("RPC chain ID {chain_id} does not match Robinhood {CHAIN_ID}");
    }
    let expected: Option<PaperExpectedPins> = args
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
    let pons_review_profile = expected
        .as_ref()
        .and_then(|pins| pins.pons_eip7702_self_batch.clone());
    validate_historical_snapshot_output_mode(
        args.verify_reviewed_boundary,
        expected
            .as_ref()
            .is_some_and(|pins| pins.pons_eip7702_self_batch.is_some()),
        args.snapshot_output.is_some(),
    )?;
    if expected.is_some() && args.bankr_proof_tx != BANKR_PROOF_TX {
        bail!("reviewed expected-pin validation requires the canonical Bankr proof transaction");
    }

    let anchor = if args.verify_reviewed_boundary {
        let reviewed = expected
            .as_ref()
            .and_then(|pins| pins.reviewed_at)
            .context("expected pins have no reviewed historical boundary")?;
        let block = rpc.block_by_number(reviewed.l2_block_number).await?;
        if pin_boundary(&block) != reviewed {
            bail!("reviewed historical block identity disagrees with canonical RPC data");
        }
        block
    } else if let Some(block) = args.l2_block {
        rpc.block_by_number(block).await?
    } else {
        let latest = rpc.latest_block().await?;
        let confirmed = latest
            .l2_block_number
            .checked_sub(args.confirmations)
            .context("latest head is below requested confirmation depth")?;
        rpc.block_by_number(confirmed).await?
    };
    let pinned_l2_block = anchor.l2_block_number;
    let observed_at = pin_boundary(&anchor);

    let mut requests = pin_requests(expected.as_ref())?;
    if !args.verify_reviewed_boundary
        && let Some(profile) = expected
            .as_ref()
            .and_then(|pins| pins.pons_eip7702_self_batch.as_ref())
    {
        requests.extend(pons_eip7702_pin_requests(profile)?);
        requests = deduplicate_requests(requests)?;
    }
    let mut pins = Vec::with_capacity(requests.len());
    let mut derived_implementations = HashSet::new();
    for request in requests {
        let code = rpc
            .code_at_l2_block(request.address, pinned_l2_block)
            .await
            .with_context(|| format!("read runtime code for {}", request.address))?;
        if code.is_empty() {
            bail!("runtime code is empty for {}", request.address);
        }
        let implementation = resolve_implementation(&rpc, request, &code, pinned_l2_block).await?;
        if implementation != request.expected_implementation {
            bail!(
                "derived implementation {implementation:?} disagrees with reviewed expectation {:?} for {}",
                request.expected_implementation,
                request.address
            );
        }
        if let Some(implementation) = implementation {
            derived_implementations.insert(implementation);
        }
        pins.push(ObservedRuntimePin {
            address: request.address,
            implementation,
            runtime_hash: keccak256(&code),
            code_bytes: Some(code.len()),
        });
    }
    let observed_addresses = pins.iter().map(|pin| pin.address).collect::<HashSet<_>>();
    for implementation in derived_implementations
        .into_iter()
        .filter(|address| !observed_addresses.contains(address))
    {
        let code = rpc
            .code_at_l2_block(implementation, pinned_l2_block)
            .await
            .with_context(|| format!("read derived implementation runtime for {implementation}"))?;
        if code.is_empty() {
            bail!("derived implementation runtime is empty for {implementation}");
        }
        pins.push(ObservedRuntimePin {
            address: implementation,
            implementation: None,
            runtime_hash: keccak256(&code),
            code_bytes: Some(code.len()),
        });
    }
    let pons_v3_semantics = Some(snapshot_pons_semantics(&rpc, pinned_l2_block).await?);
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
        schema_version: 4,
        document_role: ObservedPinsDocumentRole::ObservedStartupSnapshot,
        provenance: ObservedPinsProvenance::StartupObservation,
        fixture_id: None,
        chain_id,
        observed_at: Some(observed_at),
        pins,
        pons_v3_semantics,
        hood_protocol,
    };

    let proof = inspect_bankr_proof(&rpc, args.bankr_proof_tx).await?;
    let stable_anchor = rpc.block_by_number(pinned_l2_block).await?;
    ensure_stable_anchor(&anchor, &stable_anchor, "snapshot")?;
    let expected_validation_passed = if let Some(mut expected) = expected {
        if args.verify_reviewed_boundary {
            if let Some(profile) = expected.pons_eip7702_self_batch.as_ref() {
                verify_pons_eip7702_review_boundary(&rpc, profile).await?;
            }
            expected.pons_eip7702_self_batch = None;
        }
        PaperLaunchpadObserver::from_startup_snapshots(expected, snapshot.clone())?;
        true
    } else {
        false
    };
    let verified_boundaries = verification_boundaries(
        expected_validation_passed,
        args.verify_reviewed_boundary,
        observed_at,
        snapshot.pins.len(),
        pons_review_profile.as_ref(),
    );

    if let Some(path) = args.snapshot_output {
        write_new_json(&path, &snapshot)
            .with_context(|| format!("publish new snapshot {}", path.display()))?;
    }

    println!(
        "{}",
        serde_json::to_string(&SnapshotReport {
            record_type: "launchpad_pin_snapshot",
            chain_id,
            observed_at,
            pin_count: snapshot.pins.len(),
            verified_boundaries,
            expected_validation_passed,
            bankr_proof: proof,
            rpc_metrics: rpc.metrics(),
        })?
    );
    Ok(())
}

async fn snapshot_pons_semantics(
    rpc: &NoxaRpcClient,
    l2_block_number: u64,
) -> Result<PonsPredictionSemantics> {
    use hermes_feed::pons::{PONS_CURRENT_FACTORY, PONS_DEX_CONFIG_ID, PONS_LAUNCH_CONFIG_ID};
    use hermes_feed::pons_predict::{
        PONS_DEX_CONFIG_SELECTOR, PONS_LAUNCH_CONFIG_SELECTOR, PONS_LAUNCH_ENABLED_SELECTOR,
        PONS_LAUNCH_FEE_SELECTOR, PONS_LOCKER_SELECTOR, PONS_PREDICT_TOKEN_SELECTOR,
        PONS_TOKEN_CREATION_CODE_BYTES, PONS_TOKEN_CREATION_CODE_OFFSET, config_call,
        decode_address, decode_bool, decode_dex_config, decode_launch_config, decode_word,
        extract_creation_prefix,
    };

    let runtime = rpc
        .code_at_l2_block(PONS_CURRENT_FACTORY, l2_block_number)
        .await?;
    let prefix = extract_creation_prefix(&runtime)?;
    let launch_enabled = rpc
        .call_at_l2_block(
            PONS_CURRENT_FACTORY,
            &PONS_LAUNCH_ENABLED_SELECTOR,
            l2_block_number,
        )
        .await?;
    let launch_fee = rpc
        .call_at_l2_block(
            PONS_CURRENT_FACTORY,
            &PONS_LAUNCH_FEE_SELECTOR,
            l2_block_number,
        )
        .await?;
    let locker = rpc
        .call_at_l2_block(PONS_CURRENT_FACTORY, &PONS_LOCKER_SELECTOR, l2_block_number)
        .await?;
    let launch_config = rpc
        .call_at_l2_block(
            PONS_CURRENT_FACTORY,
            &config_call(PONS_LAUNCH_CONFIG_SELECTOR, PONS_LAUNCH_CONFIG_ID),
            l2_block_number,
        )
        .await?;
    let dex_config = rpc
        .call_at_l2_block(
            PONS_CURRENT_FACTORY,
            &config_call(PONS_DEX_CONFIG_SELECTOR, PONS_DEX_CONFIG_ID),
            l2_block_number,
        )
        .await?;
    Ok(PonsPredictionSemantics {
        factory: PONS_CURRENT_FACTORY,
        launch_enabled: decode_bool(&launch_enabled)?,
        launch_fee: decode_word(&launch_fee)?,
        locker: decode_address(&locker)?,
        token_creation_code_offset: PONS_TOKEN_CREATION_CODE_OFFSET,
        token_creation_code_bytes: PONS_TOKEN_CREATION_CODE_BYTES,
        token_creation_code_hash: keccak256(prefix),
        prediction_selector: PONS_PREDICT_TOKEN_SELECTOR,
        pool_init_code_hash: hermes_feed::robinhood::UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
        launch_config_0: decode_launch_config(&launch_config)?,
        dex_config_0: decode_dex_config(&dex_config)?,
    })
}

fn verification_boundaries(
    expected_validation_passed: bool,
    verify_reviewed_boundary: bool,
    observed_at: PinBlockBoundary,
    pin_count: usize,
    pons_profile: Option<&Eip7702SelfBatchExpectedPins>,
) -> Vec<VerifiedPinBoundary> {
    if !expected_validation_passed {
        return Vec::new();
    }
    let mut boundaries = vec![VerifiedPinBoundary {
        profile: if verify_reviewed_boundary {
            "production_global"
        } else {
            "startup_snapshot"
        },
        observed_at,
        pin_count,
    }];
    if verify_reviewed_boundary && let Some(profile) = pons_profile {
        boundaries.push(VerifiedPinBoundary {
            profile: "pons_eip7702_self_batch",
            observed_at: PinBlockBoundary {
                l2_block_number: profile.proof_l2_block_number,
                l1_block_number: profile.proof_l1_block_number,
                block_timestamp: profile.proof_block_timestamp,
                l2_block_hash: profile.proof_l2_block_hash,
            },
            pin_count: 2,
        });
    }
    boundaries
}

fn validate_historical_snapshot_output_mode(
    verify_reviewed_boundary: bool,
    has_multi_boundary_profile: bool,
    has_snapshot_output: bool,
) -> Result<()> {
    if verify_reviewed_boundary && has_multi_boundary_profile && has_snapshot_output {
        bail!("multi-boundary historical verification cannot emit one observer startup snapshot");
    }
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
        request(LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION, None),
        request(CLANKER_FACTORY, None),
        request(DOPPLER_CREATE_EMITTER, None),
        request(V4_POOL_MANAGER, None),
        request(ENTRY_POINT_V07, None),
        request(BANKR_PROOF_ACCOUNT, Some(BANKR_KERNEL_IMPLEMENTATION)),
        request(BANKR_KERNEL_IMPLEMENTATION, None),
        request(HOOD_FACTORY, None),
        eip1967_request(FLAP_PORTAL_PROXY, FLAP_PORTAL_IMPLEMENTATION),
        request(FLAP_PORTAL_IMPLEMENTATION, None),
        eip1967_request(FLAP_VAULT_PORTAL_PROXY, FLAP_VAULT_PORTAL_IMPLEMENTATION),
        request(FLAP_VAULT_PORTAL_IMPLEMENTATION, None),
    ];
    requests.extend(
        PonsAdapter::required_startup_identities()
            .iter()
            .map(|identity| request(identity.address, None)),
    );

    if let Some(expected) = expected {
        if expected.launchhood_v3_factory_runtime_hash != LAUNCHHOOD_V3_FACTORY_RUNTIME_KECCAK256
            || expected.launchhood_v3_token_implementation.address
                != LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION
            || expected.launchhood_v3_token_implementation.code_bytes
                != LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_CODE_BYTES
            || expected.launchhood_v3_token_implementation.runtime_hash
                != LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_RUNTIME_KECCAK256
        {
            bail!("reviewed LaunchHood factory/implementation identity drifted");
        }
        if let Some(profile) = &expected.hood_curve {
            profile.validate()?;
            let singleton = profile
                .identity(HoodIdentityRole::OwnerSafeSingleton)
                .context("Hood Safe singleton identity missing")?;
            requests.extend(profile.identities.iter().map(|identity| {
                if identity.role == HoodIdentityRole::OwnerSafeProxy {
                    safe_proxy_request(identity.address, singleton.address)
                } else {
                    request(identity.address, None)
                }
            }));
        }
        if let Some(configured) = expected.clanker_v4 {
            configured.expected_profile()?;
            requests.extend([
                request(CLANKER_DEPLOYER, None),
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
                request(profile.token_implementation.address, None),
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
                eip1967_request(LEAVEHOOD_FACTORY_PROXY, LEAVEHOOD_FACTORY_IMPLEMENTATION),
                request(LEAVEHOOD_FACTORY_IMPLEMENTATION, None),
            ]);
        }
        if expected.leavehood_core_proxy_runtime_hash.is_some() {
            requests.extend([
                eip1967_request(LEAVEHOOD_CORE_PROXY, LEAVEHOOD_CORE_IMPLEMENTATION),
                request(LEAVEHOOD_CORE_IMPLEMENTATION, None),
            ]);
        }
        if expected.klik_factory_runtime_hash.is_some() {
            requests.push(request(KLIK_FACTORY, None));
        }
        if expected.trench_proxy_runtime_hash.is_some()
            || expected.trench_implementation_runtime_hash.is_some()
        {
            if expected.trench_proxy_runtime_hash.is_none()
                || expected.trench_implementation_runtime_hash.is_none()
            {
                bail!("Trench proxy and implementation hashes must be configured together");
            }
            requests.extend([
                eip1967_request(TRENCH_PROXY, TRENCH_IMPLEMENTATION),
                request(TRENCH_IMPLEMENTATION, None),
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

    deduplicate_requests(requests)
}

fn pons_eip7702_pin_requests(
    profile: &hermes_feed::Eip7702SelfBatchExpectedPins,
) -> Result<Vec<PinRequest>> {
    profile.validate()?;
    Ok(vec![
        request(profile.account, Some(profile.implementation)),
        request(profile.implementation, None),
    ])
}

fn validate_pons_eip7702_review_boundary(
    profile: &hermes_feed::Eip7702SelfBatchExpectedPins,
    block: &hermes_feed::RobinhoodBlock,
    account_code: &Bytes,
    implementation_code: &Bytes,
) -> Result<()> {
    let implementation = if account_code.is_empty() {
        None
    } else {
        Some(parse_eip7702_designator(account_code)?)
    };
    validate_pons_eip7702_review_boundary_evidence(
        profile,
        block,
        account_code.len(),
        implementation,
        keccak256(account_code),
        implementation_code.len(),
        keccak256(implementation_code),
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_pons_eip7702_review_boundary_evidence(
    profile: &hermes_feed::Eip7702SelfBatchExpectedPins,
    block: &hermes_feed::RobinhoodBlock,
    account_code_bytes: usize,
    designator_implementation: Option<Address>,
    designator_hash: B256,
    implementation_code_bytes: usize,
    implementation_runtime_hash: B256,
) -> Result<()> {
    profile.validate()?;
    if block.l2_block_number != profile.proof_l2_block_number
        || block.hash != profile.proof_l2_block_hash
        || block.l1_block_number != profile.proof_l1_block_number
        || block.timestamp != profile.proof_block_timestamp
    {
        bail!("Pons EIP-7702 profile review boundary disagrees with canonical block identity");
    }
    if account_code_bytes == 0 || implementation_code_bytes == 0 {
        bail!("Pons EIP-7702 reviewed delegation pair has empty runtime code");
    }
    if designator_implementation != Some(profile.implementation)
        || designator_hash != profile.designator_hash
        || implementation_runtime_hash != profile.implementation_runtime_hash
    {
        bail!("Pons EIP-7702 reviewed delegation pair drifted at its proof boundary");
    }
    Ok(())
}

async fn verify_pons_eip7702_review_boundary(
    rpc: &NoxaRpcClient,
    profile: &hermes_feed::Eip7702SelfBatchExpectedPins,
) -> Result<()> {
    let block = rpc.block_by_number(profile.proof_l2_block_number).await?;
    let account_code = rpc
        .code_at_l2_block(profile.account, profile.proof_l2_block_number)
        .await?;
    let implementation_code = rpc
        .code_at_l2_block(profile.implementation, profile.proof_l2_block_number)
        .await?;
    validate_pons_eip7702_review_boundary(profile, &block, &account_code, &implementation_code)?;
    let stable = rpc.block_by_number(profile.proof_l2_block_number).await?;
    ensure_stable_anchor(&block, &stable, "Pons EIP-7702 reviewed profile")
}

fn deduplicate_requests(requests: Vec<PinRequest>) -> Result<Vec<PinRequest>> {
    let mut seen = HashMap::new();
    let mut unique = Vec::with_capacity(requests.len());
    for request in requests {
        if let Some(previous) = seen.insert(request.address, request) {
            if previous != request {
                bail!(
                    "conflicting implementation resolver requirements for {}",
                    request.address
                );
            }
        } else {
            unique.push(request);
        }
    }
    Ok(unique)
}

const fn request(address: Address, implementation: Option<Address>) -> PinRequest {
    PinRequest {
        address,
        expected_implementation: implementation,
        resolver: if implementation.is_some() {
            ImplementationResolver::Eip7702
        } else {
            ImplementationResolver::Direct
        },
    }
}

const fn eip1967_request(address: Address, implementation: Address) -> PinRequest {
    PinRequest {
        address,
        expected_implementation: Some(implementation),
        resolver: ImplementationResolver::Eip1967,
    }
}

const fn safe_proxy_request(address: Address, implementation: Address) -> PinRequest {
    PinRequest {
        address,
        expected_implementation: Some(implementation),
        resolver: ImplementationResolver::SafeSlot0,
    }
}

fn pin_boundary(block: &hermes_feed::RobinhoodBlock) -> PinBlockBoundary {
    PinBlockBoundary {
        l2_block_number: block.l2_block_number,
        l2_block_hash: block.hash,
        l1_block_number: block.l1_block_number,
        block_timestamp: block.timestamp,
    }
}

fn write_new_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if path.exists() {
        bail!(
            "refusing to overwrite existing evidence path {}",
            path.display()
        );
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("snapshot output has no UTF-8 file name")?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));
    let result = (|| -> Result<()> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .with_context(|| format!("create temporary snapshot {}", temporary.display()))?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, value).context("serialize snapshot")?;
        writer.write_all(b"\n").context("terminate snapshot JSON")?;
        writer.flush().context("flush snapshot")?;
        writer.get_ref().sync_all().context("sync snapshot")?;
        drop(writer);

        // hard_link is an atomic no-overwrite publication: unlike rename on
        // Unix, it fails if any file, symlink, or hard link already exists at
        // the destination, so reviewed expected-pin authority cannot be
        // truncated through an alias.
        std::fs::hard_link(&temporary, path)
            .with_context(|| format!("publish snapshot without overwrite to {}", path.display()))?;
        std::fs::remove_file(&temporary).context("remove published temporary snapshot")?;
        if let Ok(directory) = File::open(parent) {
            directory.sync_all().context("sync snapshot directory")?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn ensure_stable_anchor(
    start: &hermes_feed::RobinhoodBlock,
    end: &hermes_feed::RobinhoodBlock,
    label: &str,
) -> Result<()> {
    if start != end {
        bail!("{label} anchor reorged or changed during collection");
    }
    Ok(())
}

async fn resolve_implementation(
    rpc: &NoxaRpcClient,
    request: PinRequest,
    runtime: &Bytes,
    l2_block_number: u64,
) -> Result<Option<Address>> {
    match request.resolver {
        ImplementationResolver::Direct => {
            if request.expected_implementation.is_some() {
                bail!("direct pin unexpectedly declares an implementation");
            }
            Ok(None)
        }
        ImplementationResolver::Eip7702 => Ok(Some(parse_eip7702_designator(runtime)?)),
        ImplementationResolver::Eip1967 => {
            let word = rpc
                .storage_at_l2_block(
                    request.address,
                    EIP1967_IMPLEMENTATION_SLOT,
                    l2_block_number,
                )
                .await?;
            Ok(Some(parse_implementation_word(
                word,
                "EIP-1967 implementation",
            )?))
        }
        ImplementationResolver::SafeSlot0 => {
            let word = rpc
                .storage_at_l2_block(request.address, B256::ZERO, l2_block_number)
                .await?;
            Ok(Some(parse_implementation_word(word, "Safe singleton")?))
        }
    }
}

fn parse_eip7702_designator(runtime: &Bytes) -> Result<Address> {
    if runtime.len() != 23 || runtime.get(..3) != Some(&[0xef, 0x01, 0x00]) {
        bail!("EIP-7702 designator must be exactly ef0100 || 20-byte implementation");
    }
    let implementation = Address::from_slice(&runtime[3..]);
    if implementation == Address::ZERO {
        bail!("EIP-7702 designator has a zero implementation");
    }
    Ok(implementation)
}

fn parse_implementation_word(word: B256, label: &str) -> Result<Address> {
    if word.as_slice()[..12].iter().any(|byte| *byte != 0) {
        bail!("{label} storage word is not a canonical address");
    }
    let implementation = Address::from_slice(&word.as_slice()[12..]);
    if implementation == Address::ZERO {
        bail!("{label} storage word is zero");
    }
    Ok(implementation)
}

async fn inspect_bankr_proof(rpc: &NoxaRpcClient, transaction_hash: B256) -> Result<BankrProof> {
    let transaction = rpc
        .transaction_by_hash(transaction_hash)
        .await?
        .with_context(|| format!("Bankr proof transaction {transaction_hash} is missing"))?;
    let receipt = rpc
        .receipt(transaction_hash)
        .await?
        .with_context(|| format!("Bankr proof receipt {transaction_hash} is missing"))?;
    let transaction_block = transaction
        .l2_block_number
        .context("Bankr proof transaction has no block number")?;
    let transaction_index = transaction
        .transaction_index
        .context("Bankr proof transaction has no transaction index")?;
    if !receipt.status
        || receipt.transaction_hash != transaction_hash
        || receipt.l2_block_number != transaction_block
        || receipt.transaction_index != transaction_index
    {
        bail!("Bankr proof transaction and successful receipt identity disagree");
    }
    if transaction_hash == BANKR_PROOF_TX
        && (receipt.l2_block_number != BANKR_PROOF_L2_BLOCK
            || receipt.block_hash != BANKR_PROOF_BLOCK_HASH
            || receipt.transaction_index != BANKR_PROOF_TRANSACTION_INDEX)
    {
        bail!("canonical Bankr proof receipt boundary drifted");
    }
    let proof_anchor = rpc.block_by_number(receipt.l2_block_number).await?;
    if proof_anchor.hash != receipt.block_hash {
        bail!("Bankr proof receipt is not bound to the canonical block hash");
    }
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
    let mut successful_user_operation = 0_usize;
    for log in &receipt.logs {
        if log.address == ENTRY_POINT_V07
            && log.topics.first() == Some(&UserOperationEvent::SIGNATURE_HASH)
        {
            let event =
                UserOperationEvent::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
                    .context("decode Bankr proof UserOperationEvent")?;
            if event.sender == BANKR_PROOF_ACCOUNT && event.success {
                successful_user_operation += 1;
            }
        }
    }
    if successful_user_operation != 1 {
        bail!("Bankr proof requires exactly one successful leader UserOperationEvent");
    }
    let proof_l2_block = proof_anchor.l2_block_number;
    let entry_point_code = rpc
        .code_at_l2_block(ENTRY_POINT_V07, proof_l2_block)
        .await?;
    let account_code = rpc
        .code_at_l2_block(BANKR_PROOF_ACCOUNT, proof_l2_block)
        .await?;
    let kernel_code = rpc
        .code_at_l2_block(BANKR_KERNEL_IMPLEMENTATION, proof_l2_block)
        .await?;
    let target_code = rpc
        .code_at_l2_block(DOPPLER_CREATE_EMITTER, proof_l2_block)
        .await?;
    if entry_point_code.is_empty()
        || account_code.is_empty()
        || kernel_code.is_empty()
        || target_code.is_empty()
    {
        bail!("Bankr proof identity has empty runtime code");
    }
    validate_bankr_proof_runtime_hashes(
        keccak256(&entry_point_code),
        keccak256(&account_code),
        keccak256(&kernel_code),
        keccak256(&target_code),
    )?;
    if account_code.as_ref()
        != [
            vec![0xef, 0x01, 0x00],
            BANKR_KERNEL_IMPLEMENTATION.as_slice().to_vec(),
        ]
        .concat()
    {
        bail!("Bankr proof EIP-7702 designator bytes drifted");
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
    )
    .context("strictly decode Bankr EntryPoint/ERC-7579 proof")?;
    if decoded.leader != BANKR_PROOF_ACCOUNT || decoded.target != DOPPLER_CREATE_EMITTER {
        bail!("Bankr proof decoded an unexpected leader or target");
    }
    let selector = decoded
        .calldata
        .get(..4)
        .context("Bankr proof inner calldata has no selector")?;
    if decoded.value != alloy_primitives::U256::ZERO || selector != BANKR_DOPPLER_CREATE_SELECTOR {
        bail!("Bankr proof inner value or Doppler selector drifted");
    }
    let stable_proof_anchor = rpc.block_by_number(proof_l2_block).await?;
    ensure_stable_anchor(&proof_anchor, &stable_proof_anchor, "Bankr proof")?;
    Ok(BankrProof {
        transaction_hash,
        l2_block_number: proof_l2_block,
        block_hash: proof_anchor.hash,
        transaction_index,
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
    })
}

fn validate_bankr_proof_runtime_hashes(
    entry_point: B256,
    account: B256,
    kernel: B256,
    airlock: B256,
) -> Result<()> {
    if entry_point != ENTRY_POINT_V07_RUNTIME_HASH
        || account != BANKR_ACCOUNT_DESIGNATOR_HASH
        || kernel != BANKR_KERNEL_RUNTIME_HASH
        || airlock != BANKR_AIRLOCK_RUNTIME_HASH
    {
        bail!("Bankr proof runtime identity disagrees with reviewed commitments");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hermes_feed::{BankrDopplerExpectedProfile, ClankerV4ExpectedProfile};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn production_snapshot_requests_all_independently_pinned_runtime_dependencies() {
        let expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        let requests = pin_requests(Some(&expected)).unwrap();
        assert_eq!(requests.len(), 39);
        let pons = expected.pons_eip7702_self_batch.as_ref().unwrap();
        assert_eq!(pons_eip7702_pin_requests(pons).unwrap().len(), 2);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.address == CLANKER_DEPLOYER)
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.address == LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION)
                .count(),
            1
        );
        let bankr = BankrDopplerExpectedProfile::production();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.address == bankr.token_implementation.address)
                .count(),
            1
        );
    }

    #[test]
    fn pons_profile_uses_its_later_review_boundary_and_rejects_old_empty_account_state() {
        let expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        let profile = expected.pons_eip7702_self_batch.as_ref().unwrap();
        let proof_block = hermes_feed::RobinhoodBlock {
            l2_block_number: profile.proof_l2_block_number,
            l1_block_number: profile.proof_l1_block_number,
            timestamp: profile.proof_block_timestamp,
            hash: profile.proof_l2_block_hash,
        };
        validate_pons_eip7702_review_boundary_evidence(
            profile,
            &proof_block,
            23,
            Some(profile.implementation),
            profile.designator_hash,
            1,
            profile.implementation_runtime_hash,
        )
        .unwrap();

        let old_global_boundary = hermes_feed::RobinhoodBlock {
            l2_block_number: expected.reviewed_at.unwrap().l2_block_number,
            l1_block_number: expected.reviewed_at.unwrap().l1_block_number,
            timestamp: expected.reviewed_at.unwrap().block_timestamp,
            hash: expected.reviewed_at.unwrap().l2_block_hash,
        };
        assert!(
            validate_pons_eip7702_review_boundary_evidence(
                profile,
                &old_global_boundary,
                0,
                None,
                keccak256([]),
                0,
                keccak256([]),
            )
            .is_err()
        );
    }

    #[test]
    fn multi_boundary_historical_verifier_rejects_snapshot_output() {
        assert!(validate_historical_snapshot_output_mode(true, true, true).is_err());
        assert!(validate_historical_snapshot_output_mode(true, true, false).is_ok());
        assert!(validate_historical_snapshot_output_mode(false, true, true).is_ok());
    }

    #[test]
    fn report_exposes_every_independently_verified_historical_boundary() {
        let expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        let global = expected.reviewed_at.unwrap();
        let profile = expected.pons_eip7702_self_batch.as_ref().unwrap();

        let boundaries = verification_boundaries(true, true, global, 39, Some(profile));

        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0].profile, "production_global");
        assert_eq!(boundaries[0].observed_at, global);
        assert_eq!(boundaries[0].pin_count, 39);
        assert_eq!(boundaries[1].profile, "pons_eip7702_self_batch");
        assert_eq!(
            boundaries[1].observed_at,
            PinBlockBoundary {
                l2_block_number: profile.proof_l2_block_number,
                l1_block_number: profile.proof_l1_block_number,
                block_timestamp: profile.proof_block_timestamp,
                l2_block_hash: profile.proof_l2_block_hash,
            }
        );
        assert_eq!(boundaries[1].pin_count, 2);
    }

    #[test]
    fn fresh_report_exposes_one_complete_startup_boundary() {
        let observed_at = PinBlockBoundary {
            l2_block_number: 12,
            l1_block_number: 34,
            block_timestamp: 56,
            l2_block_hash: B256::with_last_byte(78),
        };

        let boundaries = verification_boundaries(true, false, observed_at, 41, None);

        assert_eq!(boundaries.len(), 1);
        assert_eq!(boundaries[0].profile, "startup_snapshot");
        assert_eq!(boundaries[0].observed_at, observed_at);
        assert_eq!(boundaries[0].pin_count, 41);
    }

    #[test]
    fn observation_without_expected_pins_claims_no_verified_boundary() {
        let observed_at = PinBlockBoundary {
            l2_block_number: 12,
            l1_block_number: 34,
            block_timestamp: 56,
            l2_block_hash: B256::with_last_byte(78),
        };

        assert!(verification_boundaries(false, false, observed_at, 41, None).is_empty());
    }

    #[test]
    fn snapshot_request_shape_with_clanker_deployer_reaches_startup_validation() {
        let mut expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../../tests/fixtures/launchpad-paper-expected-pins.synthetic.json"
        ))
        .unwrap();
        let existing: PaperObservedStartupSnapshot = serde_json::from_str(include_str!(
            "../../tests/fixtures/launchpad-paper-observed-startup.synthetic.json"
        ))
        .unwrap();
        let profile = ClankerV4ExpectedProfile::production();
        expected.clanker_v4 = Some(hermes_feed::paper_observer::ConfiguredClankerV4 {
            factory_runtime_hash: profile.factory.runtime_code_hash,
            deployer_runtime_hash: hermes_feed::launchpad_adapters::CLANKER_DEPLOYER_RUNTIME_HASH,
            pool_manager_runtime_hash: profile.pool_manager.runtime_code_hash,
            hook_runtime_hash: profile.hook.runtime_code_hash,
            locker_runtime_hash: profile.locker.runtime_code_hash,
            mev_module_runtime_hash: profile.mev_module.runtime_code_hash,
            extension_runtime_hash: profile.extension.runtime_code_hash,
            max_static_fee_ppm: profile.max_static_fee_ppm,
            max_mev_fee_ppm: profile.max_mev_fee_ppm,
            max_mev_seconds_to_decay: profile.max_mev_seconds_to_decay,
            mev_delay_guard_seconds: profile.mev_delay_guard_seconds,
            protocol_fee_share_percent: profile.protocol_fee_share_percent,
        });

        let requests = pin_requests(Some(&expected)).unwrap();
        let runtime_hash = |address: Address| {
            existing
                .pins
                .iter()
                .find(|pin| pin.address == address)
                .map(|pin| pin.runtime_hash)
                .or_else(|| {
                    [
                        (
                            CLANKER_DEPLOYER,
                            hermes_feed::launchpad_adapters::CLANKER_DEPLOYER_RUNTIME_HASH,
                        ),
                        (
                            profile.pool_manager.address,
                            profile.pool_manager.runtime_code_hash,
                        ),
                        (profile.hook.address, profile.hook.runtime_code_hash),
                        (profile.locker.address, profile.locker.runtime_code_hash),
                        (
                            profile.mev_module.address,
                            profile.mev_module.runtime_code_hash,
                        ),
                        (
                            profile.extension.address,
                            profile.extension.runtime_code_hash,
                        ),
                    ]
                    .into_iter()
                    .find_map(|(candidate, hash)| (candidate == address).then_some(hash))
                })
                .unwrap_or_else(|| keccak256(address.as_slice()))
        };
        let pins = requests
            .iter()
            .map(|request| {
                existing
                    .pins
                    .iter()
                    .find(|pin| pin.address == request.address)
                    .copied()
                    .unwrap_or(ObservedRuntimePin {
                        address: request.address,
                        implementation: request.expected_implementation,
                        runtime_hash: runtime_hash(request.address),
                        code_bytes: Some(1),
                    })
            })
            .collect();
        let snapshot = PaperObservedStartupSnapshot { pins, ..existing };

        assert!(snapshot.pins.iter().any(|pin| {
            pin.address == CLANKER_DEPLOYER
                && pin.runtime_hash
                    == hermes_feed::launchpad_adapters::CLANKER_DEPLOYER_RUNTIME_HASH
        }));
        PaperLaunchpadObserver::from_startup_snapshots(expected, snapshot).unwrap();
    }

    #[test]
    fn production_bankr_snapshot_requests_every_reviewed_dependency() {
        let expected: PaperExpectedPins = serde_json::from_str(include_str!(
            "../../config/launchpad-expected-pins.production.json"
        ))
        .unwrap();
        let profile = BankrDopplerExpectedProfile::production();
        let requests = pin_requests(Some(&expected)).unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.address == LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION)
                .count(),
            1
        );
        assert_eq!(
            expected.launchhood_v3_token_implementation.address,
            LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION
        );
        assert_eq!(
            expected.launchhood_v3_token_implementation.code_bytes,
            LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_CODE_BYTES
        );
        assert_eq!(
            expected.launchhood_v3_token_implementation.runtime_hash,
            LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION_RUNTIME_KECCAK256
        );
        for address in [
            profile.airlock.address,
            profile.pool_manager.address,
            profile.initializer.address,
            profile.rehype_hook.address,
            profile.token_factory.address,
            profile.token_implementation.address,
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
                .expected_implementation,
            profile
                .smart_account
                .delegation_implementation
                .map(|pin| pin.address)
        );
        assert_eq!(
            requests
                .iter()
                .find(|request| request.address == profile.smart_account.account.address)
                .unwrap()
                .resolver,
            ImplementationResolver::Eip7702
        );
        assert_eq!(
            requests
                .iter()
                .find(|request| request.address == FLAP_PORTAL_PROXY)
                .unwrap()
                .resolver,
            ImplementationResolver::Eip1967
        );
        let safe = expected
            .hood_curve
            .as_ref()
            .unwrap()
            .identity(HoodIdentityRole::OwnerSafeProxy)
            .unwrap();
        assert_eq!(
            requests
                .iter()
                .find(|request| request.address == safe.address)
                .unwrap()
                .resolver,
            ImplementationResolver::SafeSlot0
        );
        assert_eq!(
            expected
                .bankr_doppler_v4
                .unwrap()
                .expected_profile()
                .unwrap(),
            profile
        );
        assert_eq!(
            expected.reviewed_at,
            Some(PinBlockBoundary {
                l2_block_number: 10_980_306,
                l2_block_hash: alloy_primitives::b256!(
                    "918363e5b20e86dbe7e952f261a60c9882975ec434abb5815a9dbecdc6354173"
                ),
                l1_block_number: 25_542_926,
                block_timestamp: 1_784_177_178,
            })
        );
    }

    #[test]
    fn implementation_parsers_are_exact_and_nonzero() {
        let implementation = Address::with_last_byte(0x42);
        let designator =
            Bytes::from([vec![0xef, 0x01, 0x00], implementation.as_slice().to_vec()].concat());
        assert_eq!(
            parse_eip7702_designator(&designator).unwrap(),
            implementation
        );

        let mut wrong_prefix = designator.to_vec();
        wrong_prefix[0] = 0xee;
        assert!(parse_eip7702_designator(&Bytes::from(wrong_prefix)).is_err());
        let mut too_long = designator.to_vec();
        too_long.push(0);
        assert!(parse_eip7702_designator(&Bytes::from(too_long)).is_err());
        let zero_designator = Bytes::from([vec![0xef, 0x01, 0x00], vec![0_u8; 20]].concat());
        assert!(parse_eip7702_designator(&zero_designator).is_err());

        let mut word = [0_u8; 32];
        word[12..].copy_from_slice(implementation.as_slice());
        assert_eq!(
            parse_implementation_word(B256::from(word), "test").unwrap(),
            implementation
        );
        word[0] = 1;
        assert!(parse_implementation_word(B256::from(word), "test").is_err());
        assert!(parse_implementation_word(B256::ZERO, "test").is_err());
    }

    #[test]
    fn conflicting_duplicate_resolvers_fail_closed() {
        let address = Address::with_last_byte(1);
        assert!(
            deduplicate_requests(vec![
                request(address, None),
                eip1967_request(address, Address::with_last_byte(2)),
            ])
            .is_err()
        );
    }

    #[test]
    fn canonical_anchor_changes_fail_closed() {
        let start = hermes_feed::RobinhoodBlock {
            l2_block_number: 10,
            l1_block_number: 20,
            timestamp: 30,
            hash: B256::with_last_byte(40),
        };
        assert!(ensure_stable_anchor(&start, &start, "test").is_ok());
        for changed in [
            hermes_feed::RobinhoodBlock {
                hash: B256::with_last_byte(41),
                ..start.clone()
            },
            hermes_feed::RobinhoodBlock {
                l1_block_number: 21,
                ..start.clone()
            },
            hermes_feed::RobinhoodBlock {
                timestamp: 31,
                ..start.clone()
            },
        ] {
            assert!(ensure_stable_anchor(&start, &changed, "test").is_err());
        }
    }

    #[test]
    fn every_bankr_proof_runtime_commitment_is_required() {
        let reviewed = [
            ENTRY_POINT_V07_RUNTIME_HASH,
            BANKR_ACCOUNT_DESIGNATOR_HASH,
            BANKR_KERNEL_RUNTIME_HASH,
            BANKR_AIRLOCK_RUNTIME_HASH,
        ];
        assert!(
            validate_bankr_proof_runtime_hashes(reviewed[0], reviewed[1], reviewed[2], reviewed[3])
                .is_ok()
        );
        for index in 0..reviewed.len() {
            let mut drifted = reviewed;
            drifted[index] = B256::with_last_byte(0xee);
            assert!(
                validate_bankr_proof_runtime_hashes(drifted[0], drifted[1], drifted[2], drifted[3])
                    .is_err(),
                "runtime commitment {index} was not enforced"
            );
        }
    }

    #[test]
    fn snapshot_publication_never_overwrites_existing_or_hard_linked_authority() {
        let directory = tempdir().unwrap();
        let expected = directory.path().join("expected.json");
        std::fs::write(&expected, b"reviewed authority").unwrap();

        let existing = directory.path().join("existing.json");
        std::fs::write(&existing, b"existing evidence").unwrap();
        assert!(write_new_json(&existing, &serde_json::json!({"new": true})).is_err());
        assert_eq!(std::fs::read(&existing).unwrap(), b"existing evidence");

        let hard_link = directory.path().join("authority-alias.json");
        std::fs::hard_link(&expected, &hard_link).unwrap();
        assert!(write_new_json(&hard_link, &serde_json::json!({"new": true})).is_err());
        assert_eq!(std::fs::read(&expected).unwrap(), b"reviewed authority");

        let fresh = directory.path().join("fresh.json");
        write_new_json(&fresh, &serde_json::json!({"fresh": true})).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&std::fs::read(fresh).unwrap()).unwrap(),
            serde_json::json!({"fresh": true})
        );
    }
}
