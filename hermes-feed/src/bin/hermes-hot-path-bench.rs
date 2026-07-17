//! Deterministic, ignored-by-CI release harness for the pure candidate path.
//!
//! Run with:
//! `cargo run --release --manifest-path hermes-feed/Cargo.toml --bin hermes-hot-path-bench`

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashSet;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_sol_types::{SolCall, sol};
use hermes_feed::copy_policy::{ObservedCopySwap, WatchedWalletCopyPolicy};
use hermes_feed::launchpad_adapter::{
    AdapterKind, CandidateCall, FollowerPlanRequest, LaunchpadAdapter, LaunchpadId, NoxaV3Adapter,
    RouteKind, WrapperKind,
};
use hermes_feed::launchpad_adapters::{
    CLANKER_DEPLOY_SELECTOR, CLANKER_FACTORY, CLANKER_FACTORY_RUNTIME_HASH, CLANKER_LOCKER,
    ResearchStartupPins, RuntimeCodePin, V4_POOL_MANAGER, V4ActionObservationInput, V4AdapterSet,
    V4CandidateCall, build_adapter_paper_plan, normalize_v4_action, validate_clanker_market,
};
use hermes_feed::launchpad_registry::{
    BoundedCall, ContractPin as RegistryContractPin, ContractRole, DispatchKey, LaunchpadSpec,
    StartupPinSnapshot, StaticLaunchpadRegistry,
};
use hermes_feed::noxa_abi::{
    EXACT_INPUT_SINGLE_SELECTOR, V3ExactInputIntent, encode_v3_exact_input_single,
};
use hermes_feed::robinhood::{CHAIN_ID, NOXA_POOL_FEE, UNISWAP_V3_SWAP_ROUTER_02, WETH};
use hermes_feed::smart_account::{
    AccountExecutionProfile, ContractPin, ENTRY_POINT_V07, EntryPointCall, SmartAccountPin,
    SmartAccountPins, ValidatedSmartAccountPins, decode_entry_point_v07_prevalidated,
};
use hermes_feed::uniswap_v4::{
    CodePin, DYNAMIC_FEE_FLAG, FollowerV4Policy, HookPin, V4FeePolicy, V4MarketSnapshot, V4PoolKey,
    WarmV4Quote,
};

struct CountingAllocator;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);
static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn count_allocation(bytes: usize) {
    if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

sol! {
    struct BenchPackedUserOperation {
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

    function handleOps(BenchPackedUserOperation[] ops, address payable beneficiary) external;
    function execute(address dest, uint256 value, bytes func) external;
}

const TOKEN: Address = alloy_primitives::address!("6bbbb3be7424a911d5d131e272639512c1c12b07");
const LEADER: Address = alloy_primitives::address!("1000000000000000000000000000000000000001");
const SECOND_ACCOUNT: Address =
    alloy_primitives::address!("1000000000000000000000000000000000000002");
const RECIPIENT: Address = alloy_primitives::address!("2000000000000000000000000000000000000001");
const BUNDLER: Address = alloy_primitives::address!("3000000000000000000000000000000000000001");
const HOOK: Address = alloy_primitives::address!("0000000000000000000000000000000000000042");

#[derive(Clone, Copy)]
struct AllocationStats {
    allocations: f64,
    bytes: f64,
}

struct Stats {
    p50_ns: f64,
    p95_ns: f64,
    p99_ns: f64,
    allocations: AllocationStats,
}

fn main() {
    let fixtures = Fixtures::new();
    println!("case,p50_ns,p95_ns,p99_ns,allocations_per_op,allocated_bytes_per_op");

    emit("harness_overhead", measure(2_000, 100, || black_box(1_u64)));
    emit(
        "registry_lookup",
        measure(2_000, 100, || fixtures.registry_lookup()),
    );
    emit(
        "direct_v3_candidate",
        measure(2_000, 10, || fixtures.direct_v3_candidate()),
    );
    emit(
        "nested_erc4337_v4_candidate",
        measure(1_000, 1, || fixtures.nested_erc4337_v4_candidate()),
    );
    emit(
        "malformed_cross_adapter_rejection",
        measure(2_000, 100, || fixtures.cross_adapter_rejection()),
    );
    emit(
        "unchanged_noxa_candidate",
        measure(2_000, 100, || fixtures.unchanged_noxa_candidate()),
    );
    emit(
        "synchronous_boundary_emit_serialization_lower_bound",
        measure(2_000, 10, || fixtures.boundary_emit_serialization()),
    );
}

fn emit(name: &str, stats: Stats) {
    println!(
        "{name},{:.2},{:.2},{:.2},{:.3},{:.1}",
        stats.p50_ns,
        stats.p95_ns,
        stats.p99_ns,
        stats.allocations.allocations,
        stats.allocations.bytes,
    );
}

fn measure<T, F>(samples: usize, batch: usize, mut operation: F) -> Stats
where
    F: FnMut() -> T,
{
    for _ in 0..1_000 {
        black_box(operation());
    }

    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        for _ in 0..batch {
            black_box(operation());
        }
        timings.push(started.elapsed().as_nanos() as f64 / batch as f64);
    }
    timings.sort_by(f64::total_cmp);

    ALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    let allocation_batch = batch.max(100);
    COUNT_ALLOCATIONS.store(true, Ordering::Relaxed);
    for _ in 0..allocation_batch {
        black_box(operation());
    }
    COUNT_ALLOCATIONS.store(false, Ordering::Relaxed);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed) as f64 / allocation_batch as f64;
    let bytes = ALLOCATED_BYTES.load(Ordering::Relaxed) as f64 / allocation_batch as f64;

    Stats {
        p50_ns: percentile(&timings, 50),
        p95_ns: percentile(&timings, 95),
        p99_ns: percentile(&timings, 99),
        allocations: AllocationStats { allocations, bytes },
    }
}

fn percentile(sorted: &[f64], percentile: usize) -> f64 {
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

struct Fixtures {
    registry: StaticLaunchpadRegistry,
    v3_calldata: Vec<u8>,
    noxa_observed: ObservedCopySwap,
    noxa_policy: WatchedWalletCopyPolicy,
    smart_calldata: Vec<u8>,
    smart_pins: ValidatedSmartAccountPins<'static>,
    v4_adapters: V4AdapterSet,
    v4_market: V4MarketSnapshot,
    v4_quote: WarmV4Quote,
    validated_v4: hermes_feed::launchpad_adapters::ValidatedAdapterMarket,
    boundary_event: serde_json::Value,
}

impl Fixtures {
    fn new() -> Self {
        let v3_intent = V3ExactInputIntent {
            token_in: WETH,
            token_out: TOKEN,
            fee: NOXA_POOL_FEE,
            recipient: LEADER,
            amount_in: U256::from(100_u64),
            amount_out_minimum: U256::from(900_u64),
            sqrt_price_limit_x96: U256::ZERO,
        };
        let v3_calldata = encode_v3_exact_input_single(&v3_intent).expect("fixture encodes");
        let noxa_observed = ObservedCopySwap {
            tx_hash: B256::repeat_byte(0x44),
            chain_id: Some(CHAIN_ID),
            from: LEADER,
            to: UNISWAP_V3_SWAP_ROUTER_02,
            value: U256::ZERO,
            intent: v3_intent,
        };
        let noxa_policy = WatchedWalletCopyPolicy::new(
            HashSet::from([LEADER]),
            HashSet::from([TOKEN]),
            U256::from(50_u64),
            U256::from(1_000_u64),
            10,
        )
        .expect("policy fixture is valid");

        let clanker_input = [&CLANKER_DEPLOY_SELECTOR[..], &[0_u8; 32][..]].concat();
        let nested = executeCall {
            dest: CLANKER_FACTORY,
            value: U256::ZERO,
            func: clanker_input.into(),
        }
        .abi_encode();
        let outer = executeCall {
            dest: SECOND_ACCOUNT,
            value: U256::ZERO,
            func: nested.into(),
        }
        .abi_encode();
        let smart_calldata = handleOpsCall {
            ops: vec![BenchPackedUserOperation {
                sender: LEADER,
                nonce: U256::from(7_u64),
                initCode: Bytes::new(),
                callData: outer.into(),
                accountGasLimits: B256::repeat_byte(0x11),
                preVerificationGas: U256::from(21_000_u64),
                gasFees: B256::repeat_byte(0x22),
                paymasterAndData: Bytes::new(),
                signature: Bytes::from_static(&[0x12, 0x34]),
            }],
            beneficiary: RECIPIENT,
        }
        .abi_encode();
        let smart_accounts = Box::leak(Box::new([
            smart_account_pin(LEADER, 0x31),
            smart_account_pin(SECOND_ACCOUNT, 0x32),
        ]));
        let smart_targets = Box::leak(Box::new([ContractPin {
            address: CLANKER_FACTORY,
            runtime_code_hash: CLANKER_FACTORY_RUNTIME_HASH,
        }]));
        let smart_pins = ValidatedSmartAccountPins::new(SmartAccountPins {
            entry_point: ContractPin {
                address: ENTRY_POINT_V07,
                runtime_code_hash: B256::repeat_byte(0x30),
            },
            accounts: smart_accounts,
            allowed_targets: smart_targets,
        })
        .expect("startup-validated smart-account pins");
        let v4_adapters = V4AdapterSet::from_research(ResearchStartupPins::default())
            .expect("static V4 adapter fixture");
        let key = V4PoolKey::canonical(WETH, TOKEN, DYNAMIC_FEE_FLAG, 60, HOOK)
            .expect("canonical V4 fixture");
        let v4_market = V4MarketSnapshot {
            chain_id: CHAIN_ID,
            pool_manager: CodePin {
                address: V4_POOL_MANAGER,
                runtime_code_hash: B256::repeat_byte(0x51),
            },
            key,
            pool_id: key.pool_id(),
            hook: HookPin {
                code: CodePin {
                    address: HOOK,
                    runtime_code_hash: B256::repeat_byte(0x52),
                },
                configuration_hash: B256::repeat_byte(0x53),
            },
            quote_asset: WETH,
            fee_policy: V4FeePolicy::Dynamic {
                min_fee_ppm: 1_000,
                max_fee_ppm: 10_000,
            },
            state_version: 7,
        };
        let v4_quote = WarmV4Quote {
            pool_id: v4_market.pool_id,
            state_version: v4_market.state_version,
            asset_in: WETH,
            asset_out: TOKEN,
            amount_in: U256::from(100_u64),
            expected_amount_out: U256::from(1_000_u64),
            applied_fee_ppm: 5_000,
        };
        let validated_v4 = validate_clanker_market(
            RuntimeCodePin {
                address: CLANKER_FACTORY,
                runtime_code_hash: CLANKER_FACTORY_RUNTIME_HASH,
            },
            RuntimeCodePin {
                address: CLANKER_LOCKER,
                runtime_code_hash: B256::repeat_byte(0x54),
            },
            &v4_market,
        )
        .expect("validated V4 market fixture");

        Self {
            registry: combined_registry(),
            v3_calldata,
            noxa_observed,
            noxa_policy,
            smart_calldata,
            smart_pins,
            v4_adapters,
            v4_market,
            v4_quote,
            validated_v4,
            boundary_event: serde_json::json!({
                "record_type": "runtime_signed_boundary",
                "decision": {
                    "decision": "submit_now",
                    "l1_block_number": 101,
                    "l1_timestamp": 1_700_000_001_u64,
                },
                "tx_hash": B256::repeat_byte(0x44),
                "nonce": 7,
            }),
        }
    }

    fn registry_lookup(&self) -> LaunchpadId {
        self.registry
            .dispatch(
                Some(CHAIN_ID),
                BoundedCall {
                    destination: UNISWAP_V3_SWAP_ROUTER_02,
                    calldata: &self.v3_calldata,
                    wrapper: WrapperKind::Direct,
                    depth: 0,
                },
            )
            .expect("registered fixture")
            .id
    }

    fn direct_v3_candidate(&self) -> usize {
        black_box(self.registry_lookup());
        let action = NoxaV3Adapter
            .observe(CandidateCall {
                tx_hash: B256::repeat_byte(0x44),
                chain_id: Some(CHAIN_ID),
                leader: LEADER,
                destination: UNISWAP_V3_SWAP_ROUTER_02,
                value: U256::ZERO,
                calldata: &self.v3_calldata,
                wrapper: WrapperKind::Direct,
            })
            .expect("direct V3 fixture");
        NoxaV3Adapter
            .plan(FollowerPlanRequest {
                action,
                recipient: RECIPIENT,
                amount_in: U256::from(50_u64),
                min_receive: U256::from(400_u64),
            })
            .expect("paper plan fixture")
            .calldata
            .len()
    }

    fn nested_erc4337_v4_candidate(&self) -> U256 {
        let decoded = decode_entry_point_v07_prevalidated(
            EntryPointCall {
                chain_id: CHAIN_ID,
                destination: ContractPin {
                    address: ENTRY_POINT_V07,
                    runtime_code_hash: B256::repeat_byte(0x30),
                },
                outer_bundler: BUNDLER,
                calldata: &self.smart_calldata,
            },
            self.smart_pins,
        )
        .expect("nested smart-account fixture");
        self.v4_adapters
            .observe(&V4CandidateCall {
                chain_id: CHAIN_ID,
                leader: decoded.leader,
                destination: decoded.target,
                destination_runtime_hash: CLANKER_FACTORY_RUNTIME_HASH,
                implementation: None,
                value: decoded.value,
                input: &decoded.calldata,
            })
            .expect("V4 dispatch fixture");
        let action = normalize_v4_action(
            self.validated_v4,
            V4ActionObservationInput {
                launchpad: LaunchpadId::Clanker,
                attribution: None,
                leader: decoded.leader,
                asset_in: WETH,
                asset_out: TOKEN,
                observed_amount_in: U256::from(100_u64),
                observed_min_out: U256::from(800_u64),
                observed_route: decoded.calldata.as_ref(),
            },
        )
        .expect("normalized V4 fixture");
        build_adapter_paper_plan(
            self.validated_v4,
            &action,
            &self.v4_market,
            self.v4_quote,
            FollowerV4Policy {
                recipient: RECIPIENT,
                spend_limit: U256::from(120_u64),
                max_slippage_bps: 250,
            },
        )
        .expect("V4 paper plan fixture")
        .min_receive
    }

    fn cross_adapter_rejection(&self) -> bool {
        self.registry
            .dispatch(
                Some(CHAIN_ID),
                BoundedCall {
                    destination: CLANKER_FACTORY,
                    calldata: &self.v3_calldata,
                    wrapper: WrapperKind::Direct,
                    depth: 0,
                },
            )
            .is_err()
    }

    fn unchanged_noxa_candidate(&self) -> U256 {
        match self
            .noxa_policy
            .evaluate(&self.noxa_observed, None, 0)
            .expect("unchanged Noxa fixture")
        {
            hermes_feed::CopyDecision::Entry {
                follower_minimum_out,
                ..
            } => follower_minimum_out,
            _ => unreachable!("buy fixture must remain an entry"),
        }
    }

    fn boundary_emit_serialization(&self) -> usize {
        // This intentionally excludes stdout locking, formatting, and the
        // write syscall, so it is a lower bound for the synchronous emit that
        // currently runs before the submission task is spawned.
        serde_json::to_string(&self.boundary_event)
            .expect("benchmark event serializes")
            .len()
    }
}

fn smart_account_pin(address: Address, hash_byte: u8) -> SmartAccountPin {
    SmartAccountPin {
        account: ContractPin {
            address,
            runtime_code_hash: B256::repeat_byte(hash_byte),
        },
        execution_profile: AccountExecutionProfile::ExecuteAddressValueBytes,
        factory: None,
        delegation_implementation: None,
    }
}

fn combined_registry() -> StaticLaunchpadRegistry {
    let ids = [
        LaunchpadId::Noxa,
        LaunchpadId::Bow,
        LaunchpadId::LaunchHoodV3,
        LaunchpadId::Clanker,
        LaunchpadId::BankrDoppler,
        LaunchpadId::KlikFinance,
        LaunchpadId::TrenchToday,
        LaunchpadId::Pons,
        LaunchpadId::Flap,
        LaunchpadId::HoodFun,
        LaunchpadId::LeaveHood,
    ];
    let specs = ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let ordinal = u8::try_from(index + 1).expect("small fixture");
            let destination = if *id == LaunchpadId::Noxa {
                UNISWAP_V3_SWAP_ROUTER_02
            } else if *id == LaunchpadId::Clanker {
                CLANKER_FACTORY
            } else {
                Address::with_last_byte(ordinal)
            };
            let selector = if *id == LaunchpadId::Noxa {
                EXACT_INPUT_SINGLE_SELECTOR
            } else if *id == LaunchpadId::Clanker {
                CLANKER_DEPLOY_SELECTOR
            } else {
                [ordinal, 0, 0, 1]
            };
            LaunchpadSpec {
                id: *id,
                chain_id: CHAIN_ID,
                family: match id {
                    LaunchpadId::Clanker | LaunchpadId::KlikFinance => AdapterKind::UniswapV4,
                    LaunchpadId::BankrDoppler => AdapterKind::DopplerV4,
                    LaunchpadId::Flap => AdapterKind::FlapPortal,
                    LaunchpadId::HoodFun | LaunchpadId::LeaveHood => AdapterKind::NativeCurve,
                    _ => AdapterKind::V3LaunchAtBirth,
                },
                observation_keys: vec![DispatchKey {
                    destination,
                    selector,
                    wrapper: WrapperKind::Direct,
                }],
                contract_pins: vec![RegistryContractPin {
                    role: ContractRole::LaunchFactory,
                    address: destination,
                    implementation: None,
                    runtime_code_hash: B256::with_last_byte(ordinal),
                }],
                allowed_routes: vec![match id {
                    LaunchpadId::Clanker | LaunchpadId::KlikFinance => RouteKind::V4HookedPool,
                    LaunchpadId::BankrDoppler => RouteKind::DopplerPermit2,
                    LaunchpadId::Flap | LaunchpadId::HoodFun | LaunchpadId::LeaveHood => {
                        RouteKind::NativeBondingCurve
                    }
                    _ => RouteKind::V3SingleHop,
                }],
                quote_assets: vec![WETH],
            }
        })
        .collect::<Vec<_>>();
    let startup = StartupPinSnapshot {
        chain_id: CHAIN_ID,
        pins: specs
            .iter()
            .flat_map(|spec| spec.contract_pins.iter().copied())
            .map(RegistryContractPin::observed_identity)
            .collect(),
    };
    StaticLaunchpadRegistry::from_specs(startup, specs).expect("combined registry fixture")
}
