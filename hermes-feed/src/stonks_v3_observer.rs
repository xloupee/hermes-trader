//! Fail-closed, receipt-block observation for the StonksLauncherV3 direct
//! Doppler launch profile.
//!
//! This module deliberately produces reconciliation evidence only. It does
//! not expose a quote, trade plan, signer, execution path, or broadcast path.

use alloy_primitives::{Address, B256, Bytes, U256, keccak256, uint};
use alloy_sol_types::{SolCall, SolEvent};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::noxa_abi::ReceiptLog;
use crate::noxa_predict::predict_v3_pool_address;
use crate::noxa_rpc::{NoxaReceipt, NoxaRpcClient, RobinhoodBlock, RobinhoodTransaction};
use crate::robinhood::{
    CHAIN_ID, UNISWAP_V3_FACTORY, UNISWAP_V3_FACTORY_RUNTIME_KECCAK256,
    UNISWAP_V3_POOL_INIT_CODE_KECCAK256, WETH, WETH_RUNTIME_KECCAK256,
};

pub const STONKS_V3_LAUNCHER: Address =
    alloy_primitives::address!("2a71f10b41ff0882c7be2a5c0644722314976b42");
pub const STONKS_V3_AIRLOCK: Address =
    alloy_primitives::address!("eb7c034704ef8dcd2d32324c1545f62fb4ad0862");
pub const STONKS_V3_BUNDLER: Address =
    alloy_primitives::address!("ede0b5fae363232c396724fa962250fa197cc5a1");
pub const STONKS_V3_DN404_FACTORY: Address =
    alloy_primitives::address!("37a9fa204a4d3a429fded7e3469ab076c854bc9d");
pub const STONKS_V3_INITIALIZER: Address =
    alloy_primitives::address!("de8886a0019ea060b8378ee37b8a23b8117f29a3");
pub const STONKS_V3_GOVERNANCE_FACTORY: Address =
    alloy_primitives::address!("85f37f74ef2478a770318bc810177a9835911ad7");
pub const STONKS_V3_MIGRATOR: Address =
    alloy_primitives::address!("ba2f330edb16cd8056f5988d8ce19bbc63475a0e");
pub const STONKS_V3_USDG: Address =
    alloy_primitives::address!("5fc5360d0400a0fd4f2af552add042d716f1d168");
pub const STONKS_V3_PLATFORM_BENEFICIARY: Address =
    alloy_primitives::address!("48c2abe0e8b8a746464f36ef540c854e9f9c13fa");
pub const STONKS_V3_PROTOCOL_BENEFICIARY: Address =
    alloy_primitives::address!("edeaa06e2eb42a5c19ce27c6cffb36fd4fe1eda8");
pub const STONKS_V3_OWNER: Address =
    alloy_primitives::address!("2e8dccce588a2150d4b5a0fa9dd75fe72ace026f");
pub const STONKS_V3_USDG_OWNER: Address =
    alloy_primitives::address!("cfa0388f5ddf905fdc08c45c716c15dc10a14c6f");

pub const STONKS_V3_LAUNCHER_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("e6722f6abe49f622885917d215788103bbe472a57b5866e521cc301b77f23f44");
pub const STONKS_V3_AIRLOCK_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("86b37100cbe9841771c452a592985b4e921254b127a380246073b84ec953f7f8");
pub const STONKS_V3_BUNDLER_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("0da9f4b26917e40240417a28b24840d5096c631a7ac2584fb530fc28ad20e4be");
pub const STONKS_V3_DN404_FACTORY_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("9c5b2ac53dead80b31d431a6c986082daf404dce3ac829a62c000ee68b9138e9");
pub const STONKS_V3_INITIALIZER_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("cb2f44d32c3efcec55c36bfa6f165ae29777daedaba304da7ba64d40746bc841");
pub const STONKS_V3_GOVERNANCE_FACTORY_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("306dda9c8ef29935e569bb402c1626e8ef2048acd6b52a06bc3417bb4752c43f");
pub const STONKS_V3_MIGRATOR_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("7bf5115543e8e0769ceabe4da9b8e23547c9e95c1cce15d24d96f164406129e3");
pub const STONKS_V3_USDG_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("864cc9ad53b338b82da1f7cab85ab0b3d5c8861acb422b6fec63cf36234f36a6");
pub const STONKS_V3_USDG_IMPLEMENTATION: Address =
    alloy_primitives::address!("68184c449e1a8f34fa18d289737129fd27b66f8f");
pub const STONKS_V3_USDG_IMPLEMENTATION_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("3a551ac5c744af57e68a1d1431ac403c0f516ffd7d224a75746aee11fc4f3baf");
pub const STONKS_V3_USDG_OWNER_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("e616a4f66608354a45be3af2f82e909a1e000f4360eeb909defd830ee43c554b");
pub const STONKS_V3_DN404_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("131bbac7a12834d4d9b2b0e2befce43e604f69cca4176b302bf45bf4b6ddde9f");
pub const STONKS_V3_MIRROR_RUNTIME_HASH: B256 =
    alloy_primitives::b256!("0197437020014194be4cea1ca69c726b0eb25cfa13c44cf87563c253e3c821f9");
const STONKS_V3_LAUNCH_SELECTOR: [u8; 4] = [0x37, 0x6d, 0x65, 0x52];
const WETH_CURRENCY: u8 = 1;
const WETH_TICK_LOWER: i32 = -197_400;
const WETH_TICK_UPPER: i32 = -144_400;
const USDG_TICK_LOWER: i32 = -398_400;
const USDG_TICK_UPPER: i32 = -345_400;
const FEE: u32 = 10_000;
const TICK_SPACING: i32 = 200;
const UNIT: U256 = U256::from_limbs([200_376_420_520_689_664, 5_421, 0, 0]); // 1e23
const SUPPLY: U256 = uint!(1000000000000000000000000000_U256);
const INITIAL_SQRT_PRICE_X96: U256 = uint!(0x363db22b79374d1d73fc0_U256);
const CREATOR_SHARE: u128 = 500_000_000_000_000_000;
const PLATFORM_SHARE: u128 = 450_000_000_000_000_000;
const PROTOCOL_SHARE: u128 = 50_000_000_000_000_000;

mod abi {
    use alloy_sol_types::sol;

    sol! {
        struct LaunchParams {
            string name;
            string symbol;
            string slug;
            uint8 currency;
            address creator;
            bytes32 salt;
        }
        function launch(LaunchParams p) external returns (address asset, address pool);
        function launchAndBuy(LaunchParams p, bytes commands, bytes[] inputs) external payable;

        event Create(address indexed numeraire, address asset, address initializer, address pool);
        event Launched(
            address indexed asset,
            address indexed pool,
            address indexed creator,
            string symbol,
            uint8 currency,
            address platformBeneficiary
        );
        event DN404Created(
            address indexed asset,
            address indexed mirror,
            address indexed owner,
            uint256 unit
        );
        event Initialize(uint160 sqrtPriceX96, int24 tick);
        event PoolCreated(
            address indexed token0,
            address indexed token1,
            uint24 indexed fee,
            int24 tickSpacing,
            address pool
        );
        event Mint(
            address sender,
            address indexed owner,
            int24 indexed tickLower,
            int24 indexed tickUpper,
            uint128 amount,
            uint256 amount0,
            uint256 amount1
        );
        struct BeneficiaryData {
            address beneficiary;
            uint96 shares;
        }
        event Lock(address indexed pool, BeneficiaryData[] beneficiaries);

        function airlock() external view returns (address);
        function bundler() external view returns (address);
        function tokenFactory() external view returns (address);
        function v3Initializer() external view returns (address);
        function governanceFactory() external view returns (address);
        function migrator() external view returns (address);
        function usdg() external view returns (address);
        function weth() external view returns (address);
        function platformBeneficiary() external view returns (address);
        function wethTickLower() external view returns (int24);
        function wethTickUpper() external view returns (int24);
        function usdgTickLower() external view returns (int24);
        function usdgTickUpper() external view returns (int24);
        function owner() external view returns (address);
        function pendingOwner() external view returns (address);
        function mirrorERC721() external view returns (address);
        function baseERC20() external view returns (address);
        function unit() external view returns (uint256);
        function factory() external view returns (address);
        function token0() external view returns (address);
        function token1() external view returns (address);
        function fee() external view returns (uint24);
        function tickSpacing() external view returns (int24);
        function getPool(address tokenA, address tokenB, uint24 fee) external view returns (address pool);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CodePin {
    address: Address,
    runtime_hash: B256,
}

const STATIC_CODE_PINS: [CodePin; 10] = [
    CodePin {
        address: STONKS_V3_LAUNCHER,
        runtime_hash: STONKS_V3_LAUNCHER_RUNTIME_HASH,
    },
    CodePin {
        address: STONKS_V3_AIRLOCK,
        runtime_hash: STONKS_V3_AIRLOCK_RUNTIME_HASH,
    },
    CodePin {
        address: STONKS_V3_BUNDLER,
        runtime_hash: STONKS_V3_BUNDLER_RUNTIME_HASH,
    },
    CodePin {
        address: STONKS_V3_DN404_FACTORY,
        runtime_hash: STONKS_V3_DN404_FACTORY_RUNTIME_HASH,
    },
    CodePin {
        address: STONKS_V3_INITIALIZER,
        runtime_hash: STONKS_V3_INITIALIZER_RUNTIME_HASH,
    },
    CodePin {
        address: STONKS_V3_GOVERNANCE_FACTORY,
        runtime_hash: STONKS_V3_GOVERNANCE_FACTORY_RUNTIME_HASH,
    },
    CodePin {
        address: STONKS_V3_MIGRATOR,
        runtime_hash: STONKS_V3_MIGRATOR_RUNTIME_HASH,
    },
    CodePin {
        address: STONKS_V3_USDG,
        runtime_hash: STONKS_V3_USDG_RUNTIME_HASH,
    },
    CodePin {
        address: WETH,
        runtime_hash: WETH_RUNTIME_KECCAK256,
    },
    CodePin {
        address: STONKS_V3_USDG_OWNER,
        runtime_hash: STONKS_V3_USDG_OWNER_RUNTIME_HASH,
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3PositionEvidence {
    pub tick_lower: i32,
    pub tick_upper: i32,
    pub liquidity: U256,
    pub amount0: U256,
    pub amount1: U256,
    pub log_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct StonksV3ObservationEvidence {
    pub record_type: String,
    pub profile: String,
    pub tx_hash: B256,
    pub chain_id: u64,
    pub l2_block_number: u64,
    pub block_hash: B256,
    pub transaction_index: u64,
    pub leader: Address,
    pub launcher: Address,
    pub asset: Address,
    pub mirror: Address,
    pub pool: Address,
    pub creator: Address,
    pub currency: u8,
    pub numeraire: Address,
    pub initializer: Address,
    pub initialize_tick: i32,
    pub initialize_sqrt_price_x96: U256,
    pub position_count: usize,
    pub positions: Vec<StonksV3PositionEvidence>,
    pub quote_status: String,
    pub quote_blocker: String,
    pub paper_evidence_ready: bool,
    pub authorizes_canary: bool,
    pub execution_eligible: bool,
    pub broadcast: bool,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StonksV3ObserveError {
    #[error("transaction is not the exact direct StonksLauncherV3 launch envelope")]
    Envelope,
    #[error("launcher calldata is not the canonical WETH direct-launch tuple")]
    Calldata,
    #[error("leader is not an EOA at the receipt block")]
    LeaderNotEoa,
    #[error("receipt/block identity is inconsistent")]
    ReceiptBlock,
    #[error("receipt-block runtime dependency drift: {0}")]
    RuntimeDrift(Address),
    #[error("receipt-block launcher getter drift: {0}")]
    GetterDrift(&'static str),
    #[error("required Airlock Create, Stonks Launched, or DN404 event evidence is inconsistent")]
    EventIdentity,
    #[error("DN404 asset/mirror linkage is inconsistent")]
    Dn404Linkage,
    #[error("V3 pool CREATE2 identity, factory binding, or getters are inconsistent")]
    PoolLinkage,
    #[error("V3 initialization or exact eleven-position curve drifted")]
    PositionDrift,
    #[error("receipt-block RPC failed: {0}")]
    Rpc(String),
}

trait ReceiptBlockRpc {
    async fn code_at(&self, address: Address, block: u64) -> Result<Bytes, String>;
    async fn call_at(&self, address: Address, calldata: &[u8], block: u64)
    -> Result<Bytes, String>;
    async fn block_at(&self, block: u64) -> Result<RobinhoodBlock, String>;
    async fn storage_at(&self, address: Address, slot: B256, block: u64) -> Result<B256, String>;
}

impl ReceiptBlockRpc for NoxaRpcClient {
    async fn code_at(&self, address: Address, block: u64) -> Result<Bytes, String> {
        self.code_at_l2_block(address, block)
            .await
            .map_err(|e| e.to_string())
    }
    async fn call_at(
        &self,
        address: Address,
        calldata: &[u8],
        block: u64,
    ) -> Result<Bytes, String> {
        self.call_at_l2_block(address, calldata, block)
            .await
            .map_err(|e| e.to_string())
    }
    async fn block_at(&self, block: u64) -> Result<RobinhoodBlock, String> {
        self.block_by_number(block).await.map_err(|e| e.to_string())
    }
    async fn storage_at(&self, address: Address, slot: B256, block: u64) -> Result<B256, String> {
        self.storage_at_l2_block(address, slot, block)
            .await
            .map_err(|e| e.to_string())
    }
}

pub async fn observe_stonks_v3_direct_launch_at_receipt_block(
    rpc: &NoxaRpcClient,
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
) -> Result<StonksV3ObservationEvidence, StonksV3ObserveError> {
    observe_with_rpc(rpc, transaction, receipt, block).await
}

async fn observe_with_rpc<R: ReceiptBlockRpc + Sync>(
    rpc: &R,
    transaction: &RobinhoodTransaction,
    receipt: &NoxaReceipt,
    block: &RobinhoodBlock,
) -> Result<StonksV3ObservationEvidence, StonksV3ObserveError> {
    if transaction.to != Some(STONKS_V3_LAUNCHER)
        || transaction.value != U256::ZERO
        || transaction.l2_block_number != Some(receipt.l2_block_number)
        || transaction.transaction_index != Some(receipt.transaction_index)
        || !receipt.status
        || receipt.transaction_hash != transaction.hash
        || receipt.block_hash != block.hash
        || receipt.l2_block_number != block.l2_block_number
        || receipt.l1_block_number != Some(block.l1_block_number)
    {
        return Err(StonksV3ObserveError::Envelope);
    }
    let call = abi::launchCall::abi_decode(&transaction.input)
        .map_err(|_| StonksV3ObserveError::Calldata)?;
    if abi::launchCall::SELECTOR != STONKS_V3_LAUNCH_SELECTOR
        || call.abi_encode().as_slice() != transaction.input.as_ref()
        || call.p.currency != WETH_CURRENCY
        || call.p.creator != transaction.from
        || call.p.creator == Address::ZERO
        || call.p.name.is_empty()
        || call.p.symbol.is_empty()
        || !valid_slug(&call.p.slug)
    {
        return Err(StonksV3ObserveError::Calldata);
    }
    if !rpc
        .code_at(transaction.from, receipt.l2_block_number)
        .await
        .map_err(StonksV3ObserveError::Rpc)?
        .is_empty()
    {
        return Err(StonksV3ObserveError::LeaderNotEoa);
    }
    for pin in STATIC_CODE_PINS {
        let code = rpc
            .code_at(pin.address, receipt.l2_block_number)
            .await
            .map_err(StonksV3ObserveError::Rpc)?;
        if code.is_empty() || keccak256(&code) != pin.runtime_hash {
            return Err(StonksV3ObserveError::RuntimeDrift(pin.address));
        }
    }
    for authority in [STONKS_V3_OWNER, STONKS_V3_PROTOCOL_BENEFICIARY] {
        if !rpc
            .code_at(authority, receipt.l2_block_number)
            .await
            .map_err(StonksV3ObserveError::Rpc)?
            .is_empty()
        {
            return Err(StonksV3ObserveError::RuntimeDrift(authority));
        }
    }
    let implementation_slot =
        alloy_primitives::b256!("360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc");
    let implementation_word = rpc
        .storage_at(STONKS_V3_USDG, implementation_slot, receipt.l2_block_number)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if Address::from_slice(&implementation_word.as_slice()[12..]) != STONKS_V3_USDG_IMPLEMENTATION {
        return Err(StonksV3ObserveError::GetterDrift("USDG implementation"));
    }
    let implementation_code = rpc
        .code_at(STONKS_V3_USDG_IMPLEMENTATION, receipt.l2_block_number)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if implementation_code.is_empty()
        || keccak256(&implementation_code) != STONKS_V3_USDG_IMPLEMENTATION_RUNTIME_HASH
    {
        return Err(StonksV3ObserveError::RuntimeDrift(
            STONKS_V3_USDG_IMPLEMENTATION,
        ));
    }
    verify_getters(rpc, receipt.l2_block_number).await?;
    let stable = rpc
        .block_at(receipt.l2_block_number)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if stable != *block {
        return Err(StonksV3ObserveError::ReceiptBlock);
    }

    let (asset, pool, mirror, positions, initialize_tick) =
        parse_receipt(receipt, transaction.from, &call.p.symbol)?;
    verify_dynamic_linkage(rpc, receipt.l2_block_number, asset, mirror, pool).await?;
    let stable = rpc
        .block_at(receipt.l2_block_number)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if stable != *block {
        return Err(StonksV3ObserveError::ReceiptBlock);
    }

    Ok(StonksV3ObservationEvidence {
        record_type: "launchpad_stonks_v3_observation".into(),
        profile: "stonks_v3_direct_launch".into(),
        tx_hash: transaction.hash,
        chain_id: CHAIN_ID,
        l2_block_number: receipt.l2_block_number,
        block_hash: receipt.block_hash,
        transaction_index: receipt.transaction_index,
        leader: transaction.from,
        launcher: STONKS_V3_LAUNCHER,
        asset,
        mirror,
        pool,
        creator: call.p.creator,
        currency: call.p.currency,
        numeraire: WETH,
        initializer: STONKS_V3_INITIALIZER,
        initialize_tick,
        initialize_sqrt_price_x96: INITIAL_SQRT_PRICE_X96,
        position_count: positions.len(),
        positions,
        quote_status: "unsupported".into(),
        quote_blocker: "observe_only_stonks_v3_no_independent_quote_engine".into(),
        paper_evidence_ready: false,
        authorizes_canary: false,
        execution_eligible: false,
        broadcast: false,
    })
}

fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

async fn verify_getters<R: ReceiptBlockRpc + Sync>(
    rpc: &R,
    block: u64,
) -> Result<(), StonksV3ObserveError> {
    macro_rules! address_getter {
        ($call:ty, $expected:expr, $name:literal) => {{
            let raw = rpc
                .call_at(STONKS_V3_LAUNCHER, &<$call>::new(()).abi_encode(), block)
                .await
                .map_err(StonksV3ObserveError::Rpc)?;
            let value = <$call>::abi_decode_returns(&raw)
                .map_err(|_| StonksV3ObserveError::GetterDrift($name))?;
            if value != $expected {
                return Err(StonksV3ObserveError::GetterDrift($name));
            }
        }};
    }
    address_getter!(abi::airlockCall, STONKS_V3_AIRLOCK, "airlock");
    address_getter!(abi::bundlerCall, STONKS_V3_BUNDLER, "bundler");
    address_getter!(
        abi::tokenFactoryCall,
        STONKS_V3_DN404_FACTORY,
        "tokenFactory"
    );
    address_getter!(
        abi::v3InitializerCall,
        STONKS_V3_INITIALIZER,
        "v3Initializer"
    );
    address_getter!(
        abi::governanceFactoryCall,
        STONKS_V3_GOVERNANCE_FACTORY,
        "governanceFactory"
    );
    address_getter!(abi::migratorCall, STONKS_V3_MIGRATOR, "migrator");
    address_getter!(abi::usdgCall, STONKS_V3_USDG, "usdg");
    address_getter!(abi::wethCall, WETH, "weth");
    address_getter!(
        abi::platformBeneficiaryCall,
        STONKS_V3_PLATFORM_BENEFICIARY,
        "platformBeneficiary"
    );
    address_getter!(abi::ownerCall, STONKS_V3_OWNER, "launcher owner");
    address_getter!(
        abi::pendingOwnerCall,
        Address::ZERO,
        "launcher pendingOwner"
    );

    let lower = rpc
        .call_at(
            STONKS_V3_LAUNCHER,
            &abi::wethTickLowerCall::new(()).abi_encode(),
            block,
        )
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let upper = rpc
        .call_at(
            STONKS_V3_LAUNCHER,
            &abi::wethTickUpperCall::new(()).abi_encode(),
            block,
        )
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if abi::wethTickLowerCall::abi_decode_returns(&lower)
        .ok()
        .and_then(|value| i32::try_from(value).ok())
        != Some(WETH_TICK_LOWER)
        || abi::wethTickUpperCall::abi_decode_returns(&upper)
            .ok()
            .and_then(|value| i32::try_from(value).ok())
            != Some(WETH_TICK_UPPER)
    {
        return Err(StonksV3ObserveError::GetterDrift("weth band"));
    }
    let usdg_lower = rpc
        .call_at(
            STONKS_V3_LAUNCHER,
            &abi::usdgTickLowerCall::new(()).abi_encode(),
            block,
        )
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let usdg_upper = rpc
        .call_at(
            STONKS_V3_LAUNCHER,
            &abi::usdgTickUpperCall::new(()).abi_encode(),
            block,
        )
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if abi::usdgTickLowerCall::abi_decode_returns(&usdg_lower)
        .ok()
        .and_then(|value| i32::try_from(value).ok())
        != Some(USDG_TICK_LOWER)
        || abi::usdgTickUpperCall::abi_decode_returns(&usdg_upper)
            .ok()
            .and_then(|value| i32::try_from(value).ok())
            != Some(USDG_TICK_UPPER)
    {
        return Err(StonksV3ObserveError::GetterDrift("usdg band"));
    }
    let owner = rpc
        .call_at(
            STONKS_V3_AIRLOCK,
            &abi::ownerCall::new(()).abi_encode(),
            block,
        )
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if abi::ownerCall::abi_decode_returns(&owner).ok() != Some(STONKS_V3_PROTOCOL_BENEFICIARY) {
        return Err(StonksV3ObserveError::GetterDrift("airlock owner"));
    }
    let usdg_owner = rpc
        .call_at(STONKS_V3_USDG, &abi::ownerCall::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if abi::ownerCall::abi_decode_returns(&usdg_owner).ok() != Some(STONKS_V3_USDG_OWNER) {
        return Err(StonksV3ObserveError::GetterDrift("USDG owner"));
    }
    Ok(())
}

fn parse_receipt(
    receipt: &NoxaReceipt,
    creator: Address,
    symbol: &str,
) -> Result<
    (
        Address,
        Address,
        Address,
        Vec<StonksV3PositionEvidence>,
        i32,
    ),
    StonksV3ObserveError,
> {
    let creates: Vec<_> = receipt
        .logs
        .iter()
        .filter_map(|log| decode_event::<abi::Create>(log, STONKS_V3_AIRLOCK))
        .collect();
    let launched: Vec<_> = receipt
        .logs
        .iter()
        .filter_map(|log| decode_event::<abi::Launched>(log, STONKS_V3_LAUNCHER))
        .collect();
    let dn404: Vec<_> = receipt
        .logs
        .iter()
        .filter_map(|log| decode_event::<abi::DN404Created>(log, STONKS_V3_DN404_FACTORY))
        .collect();
    if creates.len() != 1 || launched.len() != 1 || dn404.len() != 1 {
        return Err(StonksV3ObserveError::EventIdentity);
    }
    let (create_log, create) = &creates[0];
    let (launch_log, launch) = &launched[0];
    let (dn_log, dn) = &dn404[0];
    if create.numeraire != WETH
        || create.initializer != STONKS_V3_INITIALIZER
        || launch.asset != create.asset
        || launch.pool != create.pool
        || launch.creator != creator
        || launch.symbol != symbol
        || launch.currency != WETH_CURRENCY
        || launch.platformBeneficiary != STONKS_V3_PLATFORM_BENEFICIARY
        || dn.asset != create.asset
        || dn.owner != STONKS_V3_AIRLOCK
        || dn.unit != SUPPLY
        || !(dn_log.log_index < create_log.log_index && create_log.log_index < launch_log.log_index)
    {
        return Err(StonksV3ObserveError::EventIdentity);
    }
    let initializes: Vec<_> = receipt
        .logs
        .iter()
        .filter_map(|log| decode_event::<abi::Initialize>(log, create.pool))
        .collect();
    let pool_created: Vec<_> = receipt
        .logs
        .iter()
        .filter_map(|log| decode_event::<abi::PoolCreated>(log, UNISWAP_V3_FACTORY))
        .collect();
    let locks: Vec<_> = receipt
        .logs
        .iter()
        .filter_map(|log| decode_event::<abi::Lock>(log, STONKS_V3_INITIALIZER))
        .collect();
    let mints: Vec<_> = receipt
        .logs
        .iter()
        .filter_map(|log| decode_event::<abi::Mint>(log, create.pool))
        .collect();
    // The direct-launch receipt-end quote is the initialized/minted state.
    // Any additional pool-emitted event (including a Swap or Burn) means that
    // assumption is incomplete and must be handled as a separately reviewed
    // profile instead of silently quoting the initialization state.
    let pool_log_count = receipt
        .logs
        .iter()
        .filter(|log| log.address == create.pool)
        .count();
    let initialize_tick = initializes
        .first()
        .and_then(|(_, event)| i32::try_from(event.tick).ok());
    if initializes.len() != 1
        || pool_created.len() != 1
        || locks.len() != 1
        || mints.len() != 11
        || pool_log_count != 12
        || initialize_tick != Some(WETH_TICK_LOWER)
    {
        return Err(StonksV3ObserveError::PositionDrift);
    }
    let pool_event = &pool_created[0];
    if pool_event.1.token0 != create.asset
        || pool_event.1.token1 != WETH
        || u32::try_from(pool_event.1.fee).ok() != Some(FEE)
        || i32::try_from(pool_event.1.tickSpacing).ok() != Some(TICK_SPACING)
        || pool_event.1.pool != create.pool
    {
        return Err(StonksV3ObserveError::PoolLinkage);
    }
    if locks[0].1.pool != create.pool
        || locks[0].1.beneficiaries.len() != 3
        || locks[0].1.beneficiaries[0].beneficiary != creator
        || locks[0].1.beneficiaries[0].shares != CREATOR_SHARE
        || locks[0].1.beneficiaries[1].beneficiary != STONKS_V3_PLATFORM_BENEFICIARY
        || locks[0].1.beneficiaries[1].shares != PLATFORM_SHARE
        || locks[0].1.beneficiaries[2].beneficiary != STONKS_V3_PROTOCOL_BENEFICIARY
        || locks[0].1.beneficiaries[2].shares != PROTOCOL_SHARE
    {
        return Err(StonksV3ObserveError::EventIdentity);
    }
    if U256::from(initializes[0].1.sqrtPriceX96) != INITIAL_SQRT_PRICE_X96 {
        return Err(StonksV3ObserveError::PositionDrift);
    }
    let expected_lowers = [
        -197_400, -192_200, -186_800, -181_600, -176_200, -171_000, -165_600, -160_400, -155_000,
        -149_800, -144_400,
    ];
    let expected_liquidity = [
        uint!(0xd337e874824d30601b_U256),
        uint!(0x118424301cff58bb532_U256),
        uint!(0x17af97258b0fee064fa_U256),
        uint!(0x2003ed986823d4bda3e_U256),
        uint!(0x2c7b0740803463e02d7_U256),
        uint!(0x3e6fd04fea88778a0bd_U256),
        uint!(0x5c0c6f336d2fca0c3d8_U256),
        uint!(0x8dae6c9e8dc828fb75e_U256),
        uint!(0xf86dc0a0b2f82f1b7f5_U256),
        uint!(0x2302c2d226824a4a3729_U256),
        uint!(0x2124c86ddee4d4e72e1a_U256),
    ];
    let expected_amount0 = [
        uint!(0x39e7139a8c08fa05ffa098_U256),
        uint!(0x39e7139a8c08fa05ffc48a_U256),
        uint!(0x39e7139a8c08fa05ffb76a_U256),
        uint!(0x39e7139a8c08fa05ffd0e1_U256),
        uint!(0x39e7139a8c08fa05fffe3f_U256),
        uint!(0x39e7139a8c08fa05fffbb6_U256),
        uint!(0x39e7139a8c08fa05fff225_U256),
        uint!(0x39e7139a8c08fa05fff1d3_U256),
        uint!(0x39e7139a8c08fa05fffc68_U256),
        uint!(0x39e7139a8c08fa05fffc72_U256),
        uint!(0xb0da228552db01b892506f_U256),
    ];
    let mut positions = Vec::with_capacity(11);
    for (index, (log, mint)) in mints.iter().enumerate() {
        let expected_upper = if index == 10 {
            887_200
        } else {
            WETH_TICK_UPPER
        };
        let tick_lower =
            i32::try_from(mint.tickLower).map_err(|_| StonksV3ObserveError::PositionDrift)?;
        let tick_upper =
            i32::try_from(mint.tickUpper).map_err(|_| StonksV3ObserveError::PositionDrift)?;
        if mint.sender != STONKS_V3_INITIALIZER
            || mint.owner != STONKS_V3_INITIALIZER
            || tick_lower != expected_lowers[index]
            || tick_upper != expected_upper
            || U256::from(mint.amount) != expected_liquidity[index]
            || mint.amount0 != expected_amount0[index]
            || mint.amount1 != U256::ZERO
            || (index > 0 && mints[index - 1].0.log_index >= log.log_index)
        {
            return Err(StonksV3ObserveError::PositionDrift);
        }
        positions.push(StonksV3PositionEvidence {
            tick_lower,
            tick_upper,
            liquidity: U256::from(mint.amount),
            amount0: mint.amount0,
            amount1: mint.amount1,
            log_index: log.log_index,
        });
    }
    if !(dn_log.log_index < pool_event.0.log_index
        && pool_event.0.log_index < initializes[0].0.log_index
        && initializes[0].0.log_index < mints[0].0.log_index
        && mints[10].0.log_index < locks[0].0.log_index
        && locks[0].0.log_index < create_log.log_index)
    {
        return Err(StonksV3ObserveError::PositionDrift);
    }
    Ok((
        create.asset,
        create.pool,
        dn.mirror,
        positions,
        initialize_tick.unwrap(),
    ))
}

async fn verify_dynamic_linkage<R: ReceiptBlockRpc + Sync>(
    rpc: &R,
    block: u64,
    asset: Address,
    mirror: Address,
    pool: Address,
) -> Result<(), StonksV3ObserveError> {
    for (address, expected) in [
        (asset, STONKS_V3_DN404_RUNTIME_HASH),
        (mirror, STONKS_V3_MIRROR_RUNTIME_HASH),
    ] {
        let code = rpc
            .code_at(address, block)
            .await
            .map_err(StonksV3ObserveError::Rpc)?;
        if code.is_empty() || keccak256(&code) != expected {
            return Err(StonksV3ObserveError::RuntimeDrift(address));
        }
    }
    let mirror_raw = rpc
        .call_at(asset, &abi::mirrorERC721Call::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let owner_raw = rpc
        .call_at(asset, &abi::ownerCall::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let unit_raw = rpc
        .call_at(asset, &abi::unitCall::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let base_raw = rpc
        .call_at(mirror, &abi::baseERC20Call::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if abi::mirrorERC721Call::abi_decode_returns(&mirror_raw).ok() != Some(mirror)
        || abi::ownerCall::abi_decode_returns(&owner_raw).ok() != Some(STONKS_V3_AIRLOCK)
        || abi::unitCall::abi_decode_returns(&unit_raw).ok() != Some(UNIT)
        || abi::baseERC20Call::abi_decode_returns(&base_raw).ok() != Some(asset)
    {
        return Err(StonksV3ObserveError::Dn404Linkage);
    }
    let factory = rpc
        .call_at(pool, &abi::factoryCall::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let token0 = rpc
        .call_at(pool, &abi::token0Call::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let token1 = rpc
        .call_at(pool, &abi::token1Call::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let fee = rpc
        .call_at(pool, &abi::feeCall::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let spacing = rpc
        .call_at(pool, &abi::tickSpacingCall::new(()).abi_encode(), block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    let canonical_pool = rpc
        .call_at(
            UNISWAP_V3_FACTORY,
            &abi::getPoolCall {
                tokenA: asset,
                tokenB: WETH,
                fee: FEE.try_into().expect("fee fits uint24"),
            }
            .abi_encode(),
            block,
        )
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if abi::factoryCall::abi_decode_returns(&factory).ok() != Some(UNISWAP_V3_FACTORY)
        || abi::token0Call::abi_decode_returns(&token0).ok() != Some(asset)
        || abi::token1Call::abi_decode_returns(&token1).ok() != Some(WETH)
        || abi::feeCall::abi_decode_returns(&fee)
            .ok()
            .and_then(|value| u32::try_from(value).ok())
            != Some(FEE)
        || abi::tickSpacingCall::abi_decode_returns(&spacing)
            .ok()
            .and_then(|value| i32::try_from(value).ok())
            != Some(TICK_SPACING)
        || abi::getPoolCall::abi_decode_returns(&canonical_pool).ok() != Some(pool)
        || predict_v3_pool_address(
            UNISWAP_V3_FACTORY,
            asset,
            WETH,
            FEE,
            UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
        ) != pool
    {
        return Err(StonksV3ObserveError::PoolLinkage);
    }
    let factory_code = rpc
        .code_at(UNISWAP_V3_FACTORY, block)
        .await
        .map_err(StonksV3ObserveError::Rpc)?;
    if keccak256(&factory_code) != UNISWAP_V3_FACTORY_RUNTIME_KECCAK256 {
        return Err(StonksV3ObserveError::RuntimeDrift(UNISWAP_V3_FACTORY));
    }
    Ok(())
}

fn decode_event<E: SolEvent>(log: &ReceiptLog, emitter: Address) -> Option<(&ReceiptLog, E)> {
    if log.address != emitter {
        return None;
    }
    E::decode_raw_log_validate(log.topics.iter().copied(), &log.data)
        .ok()
        .map(|event| (log, event))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use alloy_primitives::aliases::{I24, U24};
    use alloy_sol_types::SolValue;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn hex_u64(value: &str) -> u64 {
        u64::from_str_radix(value.trim_start_matches("0x"), 16).unwrap()
    }

    fn hex_bytes(value: &str) -> Bytes {
        Bytes::from(hex::decode(value.trim_start_matches("0x")).unwrap())
    }

    fn proof_fixture() -> (RobinhoodTransaction, NoxaReceipt, RobinhoodBlock) {
        let value: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/stonks-v3-direct-launch-fresh-rpc-proof.json"
        ))
        .unwrap();
        let transaction = &value["transaction"];
        let receipt = &value["receipt"];
        let block = &value["block"];
        let logs = receipt["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|log| ReceiptLog {
                address: log["address"].as_str().unwrap().parse().unwrap(),
                topics: log["topics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|topic| topic.as_str().unwrap().parse().unwrap())
                    .collect(),
                data: hex_bytes(log["data"].as_str().unwrap()),
                log_index: hex_u64(log["logIndex"].as_str().unwrap()),
            })
            .collect();
        (
            RobinhoodTransaction {
                hash: transaction["hash"].as_str().unwrap().parse().unwrap(),
                from: transaction["from"].as_str().unwrap().parse().unwrap(),
                to: Some(transaction["to"].as_str().unwrap().parse().unwrap()),
                input: hex_bytes(transaction["input"].as_str().unwrap()),
                value: U256::from_str_radix(
                    transaction["value"]
                        .as_str()
                        .unwrap()
                        .trim_start_matches("0x"),
                    16,
                )
                .unwrap(),
                l2_block_number: Some(hex_u64(transaction["blockNumber"].as_str().unwrap())),
                transaction_index: Some(hex_u64(transaction["transactionIndex"].as_str().unwrap())),
            },
            NoxaReceipt {
                transaction_hash: receipt["transactionHash"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .unwrap(),
                block_hash: receipt["blockHash"].as_str().unwrap().parse().unwrap(),
                status: receipt["status"].as_str().unwrap() == "0x1",
                l2_block_number: hex_u64(receipt["blockNumber"].as_str().unwrap()),
                l1_block_number: Some(hex_u64(receipt["l1BlockNumber"].as_str().unwrap())),
                transaction_index: hex_u64(receipt["transactionIndex"].as_str().unwrap()),
                gas_used: Some(hex_u64(receipt["gasUsed"].as_str().unwrap())),
                effective_gas_price: Some(
                    U256::from_str_radix(
                        receipt["effectiveGasPrice"]
                            .as_str()
                            .unwrap()
                            .trim_start_matches("0x"),
                        16,
                    )
                    .unwrap(),
                ),
                logs,
            },
            RobinhoodBlock {
                l2_block_number: hex_u64(block["number"].as_str().unwrap()),
                l1_block_number: hex_u64(block["l1BlockNumber"].as_str().unwrap()),
                timestamp: hex_u64(block["timestamp"].as_str().unwrap()),
                hash: block["hash"].as_str().unwrap().parse().unwrap(),
            },
        )
    }

    struct NoRpc;
    impl ReceiptBlockRpc for NoRpc {
        async fn code_at(&self, _: Address, _: u64) -> Result<Bytes, String> {
            Err("unexpected RPC".into())
        }
        async fn call_at(&self, _: Address, _: &[u8], _: u64) -> Result<Bytes, String> {
            Err("unexpected RPC".into())
        }
        async fn block_at(&self, _: u64) -> Result<RobinhoodBlock, String> {
            Err("unexpected RPC".into())
        }
        async fn storage_at(&self, _: Address, _: B256, _: u64) -> Result<B256, String> {
            Err("unexpected RPC".into())
        }
    }

    struct SmartAccountRpc {
        leader: Address,
    }
    impl ReceiptBlockRpc for SmartAccountRpc {
        async fn code_at(&self, address: Address, _: u64) -> Result<Bytes, String> {
            if address == self.leader {
                Ok(Bytes::from(vec![1]))
            } else {
                Err("unexpected RPC".into())
            }
        }
        async fn call_at(&self, _: Address, _: &[u8], _: u64) -> Result<Bytes, String> {
            Err("unexpected RPC".into())
        }
        async fn block_at(&self, _: u64) -> Result<RobinhoodBlock, String> {
            Err("unexpected RPC".into())
        }
        async fn storage_at(&self, _: Address, _: B256, _: u64) -> Result<B256, String> {
            Err("unexpected RPC".into())
        }
    }

    struct FixtureRpc {
        block: RobinhoodBlock,
        codes: HashMap<Address, Bytes>,
        calls: HashMap<(Address, [u8; 4]), Bytes>,
        storage: B256,
        block_reads: AtomicUsize,
        unstable_after: Option<usize>,
    }

    impl FixtureRpc {
        fn new(
            transaction: &RobinhoodTransaction,
            receipt: &NoxaReceipt,
            block: &RobinhoodBlock,
        ) -> Self {
            let runtime: Value = serde_json::from_str(include_str!(
                "../tests/fixtures/stonks-v3-direct-launch-runtime-code.json"
            ))
            .unwrap();
            let codes = runtime["contracts"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(address, code)| {
                    (address.parse().unwrap(), hex_bytes(code.as_str().unwrap()))
                })
                .collect();
            let launch = abi::launchCall::abi_decode(&transaction.input).unwrap();
            let (asset, pool, mirror, _, _) =
                parse_receipt(receipt, transaction.from, &launch.p.symbol).unwrap();
            let mut calls = HashMap::new();
            macro_rules! response {
                ($address:expr, $call:ty, $value:expr) => {
                    calls.insert(($address, <$call>::SELECTOR), ($value).abi_encode().into());
                };
            }
            response!(STONKS_V3_LAUNCHER, abi::airlockCall, STONKS_V3_AIRLOCK);
            response!(STONKS_V3_LAUNCHER, abi::bundlerCall, STONKS_V3_BUNDLER);
            response!(
                STONKS_V3_LAUNCHER,
                abi::tokenFactoryCall,
                STONKS_V3_DN404_FACTORY
            );
            response!(
                STONKS_V3_LAUNCHER,
                abi::v3InitializerCall,
                STONKS_V3_INITIALIZER
            );
            response!(
                STONKS_V3_LAUNCHER,
                abi::governanceFactoryCall,
                STONKS_V3_GOVERNANCE_FACTORY
            );
            response!(STONKS_V3_LAUNCHER, abi::migratorCall, STONKS_V3_MIGRATOR);
            response!(STONKS_V3_LAUNCHER, abi::usdgCall, STONKS_V3_USDG);
            response!(STONKS_V3_LAUNCHER, abi::wethCall, WETH);
            response!(
                STONKS_V3_LAUNCHER,
                abi::platformBeneficiaryCall,
                STONKS_V3_PLATFORM_BENEFICIARY
            );
            response!(STONKS_V3_LAUNCHER, abi::ownerCall, STONKS_V3_OWNER);
            response!(STONKS_V3_LAUNCHER, abi::pendingOwnerCall, Address::ZERO);
            response!(
                STONKS_V3_LAUNCHER,
                abi::wethTickLowerCall,
                I24::try_from(WETH_TICK_LOWER).unwrap()
            );
            response!(
                STONKS_V3_LAUNCHER,
                abi::wethTickUpperCall,
                I24::try_from(WETH_TICK_UPPER).unwrap()
            );
            response!(
                STONKS_V3_LAUNCHER,
                abi::usdgTickLowerCall,
                I24::try_from(USDG_TICK_LOWER).unwrap()
            );
            response!(
                STONKS_V3_LAUNCHER,
                abi::usdgTickUpperCall,
                I24::try_from(USDG_TICK_UPPER).unwrap()
            );
            response!(
                STONKS_V3_AIRLOCK,
                abi::ownerCall,
                STONKS_V3_PROTOCOL_BENEFICIARY
            );
            response!(STONKS_V3_USDG, abi::ownerCall, STONKS_V3_USDG_OWNER);
            response!(asset, abi::mirrorERC721Call, mirror);
            response!(asset, abi::ownerCall, STONKS_V3_AIRLOCK);
            response!(asset, abi::unitCall, UNIT);
            response!(mirror, abi::baseERC20Call, asset);
            response!(pool, abi::factoryCall, UNISWAP_V3_FACTORY);
            response!(pool, abi::token0Call, asset);
            response!(pool, abi::token1Call, WETH);
            response!(pool, abi::feeCall, U24::from(FEE));
            response!(
                pool,
                abi::tickSpacingCall,
                I24::try_from(TICK_SPACING).unwrap()
            );
            response!(UNISWAP_V3_FACTORY, abi::getPoolCall, pool);
            let mut storage = [0_u8; 32];
            storage[12..].copy_from_slice(STONKS_V3_USDG_IMPLEMENTATION.as_slice());
            Self {
                block: block.clone(),
                codes,
                calls,
                storage: B256::from(storage),
                block_reads: AtomicUsize::new(0),
                unstable_after: None,
            }
        }
    }

    impl ReceiptBlockRpc for FixtureRpc {
        async fn code_at(&self, address: Address, block: u64) -> Result<Bytes, String> {
            if block != self.block.l2_block_number {
                return Err("wrong historical code block".into());
            }
            self.codes
                .get(&address)
                .cloned()
                .ok_or_else(|| format!("unexpected code {address}"))
        }
        async fn call_at(
            &self,
            address: Address,
            calldata: &[u8],
            block: u64,
        ) -> Result<Bytes, String> {
            if block != self.block.l2_block_number || calldata.len() < 4 {
                return Err("wrong historical call block".into());
            }
            self.calls
                .get(&(address, calldata[..4].try_into().unwrap()))
                .cloned()
                .ok_or_else(|| format!("unexpected call {address}"))
        }
        async fn block_at(&self, block: u64) -> Result<RobinhoodBlock, String> {
            if block != self.block.l2_block_number {
                return Err("wrong historical block read".into());
            }
            let read = self.block_reads.fetch_add(1, Ordering::SeqCst) + 1;
            let mut block = self.block.clone();
            if self.unstable_after.is_some_and(|after| read >= after) {
                block.hash = B256::with_last_byte(1);
            }
            Ok(block)
        }
        async fn storage_at(
            &self,
            address: Address,
            slot: B256,
            block: u64,
        ) -> Result<B256, String> {
            if address != STONKS_V3_USDG
                || slot
                    != alloy_primitives::b256!(
                        "360894a13ba1a3210667c828492db98dca3e2076cc3735a920a3ca505d382bbc"
                    )
                || block != self.block.l2_block_number
            {
                return Err("wrong historical storage request".into());
            }
            Ok(self.storage)
        }
    }

    pub(crate) async fn fixture_observation() -> StonksV3ObservationEvidence {
        let (transaction, receipt, block) = proof_fixture();
        let rpc = FixtureRpc::new(&transaction, &receipt, &block);
        observe_with_rpc(&rpc, &transaction, &receipt, &block)
            .await
            .unwrap()
    }

    async fn spawn_fixture_json_rpc(
        fixture: FixtureRpc,
    ) -> (NoxaRpcClient, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let client = NoxaRpcClient::with_url(url).unwrap();
        let requests = Arc::new(AtomicUsize::new(0));
        let observed_requests = Arc::clone(&requests);
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let header_end = loop {
                    let mut chunk = [0_u8; 8 * 1024];
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert_ne!(read, 0);
                    request.extend_from_slice(&chunk[..read]);
                    if let Some(offset) =
                        request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        break offset + 4;
                    }
                };
                let headers = std::str::from_utf8(&request[..header_end]).unwrap();
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                while request.len() < header_end + content_length {
                    let mut chunk = [0_u8; 8 * 1024];
                    let read = stream.read(&mut chunk).await.unwrap();
                    assert_ne!(read, 0);
                    request.extend_from_slice(&chunk[..read]);
                }
                let body: Value =
                    serde_json::from_slice(&request[header_end..header_end + content_length])
                        .unwrap();
                let params = body["params"].as_array().unwrap();
                let result = match body["method"].as_str().unwrap() {
                    "eth_getCode" => {
                        let address: Address = serde_json::from_value(params[0].clone()).unwrap();
                        let code = fixture.codes.get(&address).unwrap();
                        Value::String(format!("0x{}", hex::encode(code)))
                    }
                    "eth_getStorageAt" => Value::String(format!("{:#x}", fixture.storage)),
                    "eth_call" => {
                        let call = params[0].as_object().unwrap();
                        let address: Address = serde_json::from_value(call["to"].clone()).unwrap();
                        let calldata =
                            hex::decode(call["data"].as_str().unwrap().trim_start_matches("0x"))
                                .unwrap();
                        let selector: [u8; 4] = calldata[..4].try_into().unwrap();
                        let response = fixture.calls.get(&(address, selector)).unwrap();
                        Value::String(format!("0x{}", hex::encode(response)))
                    }
                    "eth_getBlockByNumber" => serde_json::json!({
                        "number": format!("0x{:x}", fixture.block.l2_block_number),
                        "l1BlockNumber": format!("0x{:x}", fixture.block.l1_block_number),
                        "timestamp": format!("0x{:x}", fixture.block.timestamp),
                        "hash": fixture.block.hash,
                    }),
                    method => panic!("unexpected fixture RPC method {method}"),
                };
                observed_requests.fetch_add(1, Ordering::SeqCst);
                let response = serde_json::to_vec(&serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": body["id"].clone(),
                    "result": result,
                }))
                .unwrap();
                let headers = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    response.len()
                );
                stream.write_all(headers.as_bytes()).await.unwrap();
                stream.write_all(&response).await.unwrap();
                stream.shutdown().await.unwrap();
            }
        });
        (client, requests, server)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concrete_noxa_rpc_loopback_proves_observation_then_independent_quote() {
        let (transaction, receipt, block) = proof_fixture();
        let fixture = FixtureRpc::new(&transaction, &receipt, &block);
        let (rpc, requests, server) = spawn_fixture_json_rpc(fixture).await;
        let evidence =
            observe_stonks_v3_direct_launch_at_receipt_block(&rpc, &transaction, &receipt, &block)
                .await
                .unwrap();
        let quote = crate::stonks_v3_receipt_quote::quote_stonks_v3_observation(&evidence).unwrap();
        assert!(requests.load(Ordering::SeqCst) > 40);
        assert_eq!(
            rpc.metrics().logical_requests as usize,
            requests.load(Ordering::SeqCst)
        );
        assert_eq!(quote.tx_hash, transaction.hash);
        assert!(quote.entry.expected_output > U256::ZERO);
        assert!(quote.full_position_exit.expected_output > U256::ZERO);
        assert!(!quote.execution_eligible);
        assert!(!quote.broadcast);
        server.abort();
    }

    #[test]
    fn launch_selectors_are_isolated_and_profile_is_observe_only() {
        assert_eq!(abi::launchCall::SELECTOR, STONKS_V3_LAUNCH_SELECTOR);
        assert_ne!(abi::launchAndBuyCall::SELECTOR, abi::launchCall::SELECTOR);
        assert!(valid_slug("le-bald-62751894"));
        assert!(!valid_slug("../escape"));
    }

    #[test]
    fn fixed_curve_has_exact_eleven_ranges() {
        let lower = [
            -197_400, -192_200, -186_800, -181_600, -176_200, -171_000, -165_600, -160_400,
            -155_000, -149_800, -144_400,
        ];
        assert_eq!(lower.len(), 11);
        assert!(lower.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(lower[10], WETH_TICK_UPPER);
    }

    #[test]
    fn fresh_receipt_fixture_proves_exact_events_curve_and_linkage() {
        let (transaction, receipt, _) = proof_fixture();
        let launch = abi::launchCall::abi_decode(&transaction.input).unwrap();
        let (asset, pool, mirror, positions, tick) =
            parse_receipt(&receipt, transaction.from, &launch.p.symbol).unwrap();
        assert_eq!(
            asset,
            alloy_primitives::address!("01c1f35f9463c39ea9169334a64c0ed3a340ac85")
        );
        assert_eq!(
            mirror,
            alloy_primitives::address!("076e84f5fe694d927e56ce431c7a6e5dc2322d79")
        );
        assert_eq!(
            pool,
            alloy_primitives::address!("db229bb27473c07bd91bccba38ef55e5503c8288")
        );
        assert_eq!(tick, WETH_TICK_LOWER);
        assert_eq!(positions.len(), 11);
        assert_eq!(positions[0].tick_lower, -197_400);
        assert_eq!(positions[10].tick_upper, 887_200);
        assert!(
            positions
                .iter()
                .all(|position| position.amount1 == U256::ZERO)
        );
    }

    #[test]
    fn receipt_event_count_identity_order_and_position_drift_fail_closed() {
        let (transaction, receipt, _) = proof_fixture();
        let symbol = abi::launchCall::abi_decode(&transaction.input)
            .unwrap()
            .p
            .symbol;
        let create_index = receipt
            .logs
            .iter()
            .position(|log| log.address == STONKS_V3_AIRLOCK)
            .unwrap();
        let mint_index = receipt
            .logs
            .iter()
            .position(|log| {
                log.address
                    == alloy_primitives::address!("db229bb27473c07bd91bccba38ef55e5503c8288")
                    && log.topics.first() == Some(&abi::Mint::SIGNATURE_HASH)
            })
            .unwrap();
        let lock_index = receipt
            .logs
            .iter()
            .position(|log| {
                log.address == STONKS_V3_INITIALIZER
                    && log.topics.first() == Some(&abi::Lock::SIGNATURE_HASH)
            })
            .unwrap();
        let mut duplicate = receipt.clone();
        duplicate.logs.push(duplicate.logs[create_index].clone());
        assert_eq!(
            parse_receipt(&duplicate, transaction.from, &symbol),
            Err(StonksV3ObserveError::EventIdentity)
        );
        let mut wrong_emitter = receipt.clone();
        wrong_emitter.logs[create_index].address = Address::with_last_byte(1);
        assert_eq!(
            parse_receipt(&wrong_emitter, transaction.from, &symbol),
            Err(StonksV3ObserveError::EventIdentity)
        );
        let mut wrong_position = receipt.clone();
        wrong_position.logs[mint_index].topics[2] = B256::ZERO;
        assert_eq!(
            parse_receipt(&wrong_position, transaction.from, &symbol),
            Err(StonksV3ObserveError::PositionDrift)
        );
        let mut wrong_amount = receipt.clone();
        let mut mutated = wrong_amount.logs[mint_index].data.to_vec();
        *mutated.last_mut().unwrap() ^= 1;
        wrong_amount.logs[mint_index].data = mutated.into();
        assert_eq!(
            parse_receipt(&wrong_amount, transaction.from, &symbol),
            Err(StonksV3ObserveError::PositionDrift)
        );
        let mut wrong_lock = receipt.clone();
        let mut mutated = wrong_lock.logs[lock_index].data.to_vec();
        mutated[95] ^= 1;
        wrong_lock.logs[lock_index].data = mutated.into();
        assert_eq!(
            parse_receipt(&wrong_lock, transaction.from, &symbol),
            Err(StonksV3ObserveError::EventIdentity)
        );
        let mut wrong_order = receipt.clone();
        wrong_order.logs[mint_index].log_index = 32;
        assert_eq!(
            parse_receipt(&wrong_order, transaction.from, &symbol),
            Err(StonksV3ObserveError::PositionDrift)
        );
        let mut extra_pool_event = receipt.clone();
        let mut unknown = extra_pool_event.logs[mint_index].clone();
        unknown.log_index = 31;
        unknown.topics[0] = B256::with_last_byte(9);
        extra_pool_event.logs.push(unknown);
        assert_eq!(
            parse_receipt(&extra_pool_event, transaction.from, &symbol),
            Err(StonksV3ObserveError::PositionDrift)
        );
    }

    #[tokio::test]
    async fn envelope_calldata_currency_creator_and_smart_accounts_fail_closed_before_dependencies()
    {
        let (transaction, receipt, block) = proof_fixture();
        let mut wrong_target = transaction.clone();
        wrong_target.to = Some(Address::with_last_byte(1));
        assert_eq!(
            observe_with_rpc(&NoRpc, &wrong_target, &receipt, &block).await,
            Err(StonksV3ObserveError::Envelope)
        );
        let mut nonzero = transaction.clone();
        nonzero.value = U256::from(1);
        assert_eq!(
            observe_with_rpc(&NoRpc, &nonzero, &receipt, &block).await,
            Err(StonksV3ObserveError::Envelope)
        );
        let mut wrong_selector = transaction.clone();
        let mut mutated = wrong_selector.input.to_vec();
        mutated[0] ^= 1;
        wrong_selector.input = mutated.into();
        assert_eq!(
            observe_with_rpc(&NoRpc, &wrong_selector, &receipt, &block).await,
            Err(StonksV3ObserveError::Calldata)
        );
        let launch = abi::launchCall::abi_decode(&transaction.input).unwrap();
        let mut wrong_creator = launch.clone();
        wrong_creator.p.creator = Address::with_last_byte(2);
        let mut creator_tx = transaction.clone();
        creator_tx.input = wrong_creator.abi_encode().into();
        assert_eq!(
            observe_with_rpc(&NoRpc, &creator_tx, &receipt, &block).await,
            Err(StonksV3ObserveError::Calldata)
        );
        let mut wrong_currency = launch.clone();
        wrong_currency.p.currency = 0;
        let mut currency_tx = transaction.clone();
        currency_tx.input = wrong_currency.abi_encode().into();
        assert_eq!(
            observe_with_rpc(&NoRpc, &currency_tx, &receipt, &block).await,
            Err(StonksV3ObserveError::Calldata)
        );
        let launch_and_buy = abi::launchAndBuyCall {
            p: launch.p,
            commands: Bytes::new(),
            inputs: vec![],
        };
        let mut bundled = transaction.clone();
        bundled.input = launch_and_buy.abi_encode().into();
        assert_eq!(
            observe_with_rpc(&NoRpc, &bundled, &receipt, &block).await,
            Err(StonksV3ObserveError::Calldata)
        );
        assert_eq!(
            observe_with_rpc(
                &SmartAccountRpc {
                    leader: transaction.from
                },
                &transaction,
                &receipt,
                &block
            )
            .await,
            Err(StonksV3ObserveError::LeaderNotEoa)
        );
        let mut wrong_l1 = block.clone();
        wrong_l1.l1_block_number += 1;
        assert_eq!(
            observe_with_rpc(&NoRpc, &transaction, &receipt, &wrong_l1).await,
            Err(StonksV3ObserveError::Envelope)
        );
    }

    #[tokio::test]
    async fn hermetic_receipt_block_rpc_proof_is_observe_only_and_reads_two_stable_blocks() {
        let (transaction, receipt, block) = proof_fixture();
        let rpc = FixtureRpc::new(&transaction, &receipt, &block);
        let evidence = observe_with_rpc(&rpc, &transaction, &receipt, &block)
            .await
            .unwrap();
        assert_eq!(rpc.block_reads.load(Ordering::SeqCst), 2);
        assert_eq!(evidence.profile, "stonks_v3_direct_launch");
        assert_eq!(evidence.position_count, 11);
        assert_eq!(evidence.quote_status, "unsupported");
        assert!(!evidence.paper_evidence_ready);
        assert!(!evidence.authorizes_canary);
        assert!(!evidence.execution_eligible);
        assert!(!evidence.broadcast);
    }

    #[tokio::test]
    async fn every_runtime_dependency_and_stability_read_fails_closed() {
        let (transaction, receipt, block) = proof_fixture();
        let launch = abi::launchCall::abi_decode(&transaction.input).unwrap();
        let (_, pool, _, _, _) =
            parse_receipt(&receipt, transaction.from, &launch.p.symbol).unwrap();
        let baseline = FixtureRpc::new(&transaction, &receipt, &block);
        let addresses: Vec<_> = baseline
            .codes
            .keys()
            .copied()
            .filter(|address| *address != transaction.from && *address != pool)
            .collect();
        for address in addresses {
            let mut rpc = FixtureRpc::new(&transaction, &receipt, &block);
            let mut code = rpc.codes[&address].to_vec();
            if code.is_empty() {
                code.push(1);
            } else {
                code[0] ^= 1;
            }
            rpc.codes.insert(address, code.into());
            assert!(
                matches!(observe_with_rpc(&rpc, &transaction, &receipt, &block).await, Err(StonksV3ObserveError::RuntimeDrift(drift)) if drift == address),
                "runtime {address}"
            );
        }
        let mut wrong_slot = FixtureRpc::new(&transaction, &receipt, &block);
        wrong_slot.storage = B256::ZERO;
        assert_eq!(
            observe_with_rpc(&wrong_slot, &transaction, &receipt, &block).await,
            Err(StonksV3ObserveError::GetterDrift("USDG implementation"))
        );
        let mut first_reorg = FixtureRpc::new(&transaction, &receipt, &block);
        first_reorg.unstable_after = Some(1);
        assert_eq!(
            observe_with_rpc(&first_reorg, &transaction, &receipt, &block).await,
            Err(StonksV3ObserveError::ReceiptBlock)
        );
        let mut second_reorg = FixtureRpc::new(&transaction, &receipt, &block);
        second_reorg.unstable_after = Some(2);
        assert_eq!(
            observe_with_rpc(&second_reorg, &transaction, &receipt, &block).await,
            Err(StonksV3ObserveError::ReceiptBlock)
        );
    }

    #[tokio::test]
    async fn every_receipt_block_getter_and_dynamic_link_fails_closed() {
        let (transaction, receipt, block) = proof_fixture();
        let baseline = FixtureRpc::new(&transaction, &receipt, &block);
        let keys: Vec<_> = baseline.calls.keys().copied().collect();
        for key in keys {
            let mut rpc = FixtureRpc::new(&transaction, &receipt, &block);
            let replacement = if rpc.calls[&key].iter().all(|byte| *byte == 0) {
                vec![1_u8; 32]
            } else {
                vec![0_u8; 32]
            };
            rpc.calls.insert(key, replacement.into());
            assert!(
                observe_with_rpc(&rpc, &transaction, &receipt, &block)
                    .await
                    .is_err(),
                "getter {} 0x{}",
                key.0,
                hex::encode(key.1)
            );
        }
    }

    #[tokio::test]
    #[ignore = "explicit public read-only receipt-block proof; not part of hermetic CI"]
    async fn fresh_public_rpc_proof_is_observe_only() {
        let (transaction, receipt, block) = proof_fixture();
        let rpc = NoxaRpcClient::with_url(crate::robinhood::PUBLIC_RPC_URL).unwrap();
        let evidence =
            observe_stonks_v3_direct_launch_at_receipt_block(&rpc, &transaction, &receipt, &block)
                .await
                .unwrap();
        assert_eq!(evidence.profile, "stonks_v3_direct_launch");
        assert_eq!(evidence.quote_status, "unsupported");
        assert!(!evidence.paper_evidence_ready);
        assert!(!evidence.authorizes_canary);
        assert!(!evidence.execution_eligible);
        assert!(!evidence.broadcast);
    }

    #[tokio::test]
    #[ignore = "explicit public read-only historical proof set; not part of hermetic CI"]
    async fn historical_weth_proofs_pass_and_usdg_proofs_remain_unsupported() {
        let rpc = NoxaRpcClient::with_url(crate::robinhood::PUBLIC_RPC_URL).unwrap();
        let weth = [
            alloy_primitives::b256!(
                "47d4825051b72ba5a54ef4c5d5517ee08b8567a77745c93ced914d9676d3a841"
            ),
            alloy_primitives::b256!(
                "e457180cee58cb038345782353f5837532ea3fc5f62d79258e6b4e69f96649f3"
            ),
            alloy_primitives::b256!(
                "35eada5401f9d39f121229a67725aff937254960da1c45859f4584845dd39738"
            ),
        ];
        for tx_hash in weth {
            let transaction = rpc.transaction_by_hash(tx_hash).await.unwrap().unwrap();
            let receipt = rpc.receipt(tx_hash).await.unwrap().unwrap();
            let block = rpc.block_by_number(receipt.l2_block_number).await.unwrap();
            let evidence = observe_stonks_v3_direct_launch_at_receipt_block(
                &rpc,
                &transaction,
                &receipt,
                &block,
            )
            .await
            .unwrap();
            assert_eq!(evidence.profile, "stonks_v3_direct_launch");
            assert_eq!(evidence.position_count, 11);
            assert_eq!(evidence.quote_status, "unsupported");
        }
        let usdg = [
            alloy_primitives::b256!(
                "93ac8ad387afbd2d7a69425262c95db4d574d5325184722a5bb141f0b3767ab1"
            ),
            alloy_primitives::b256!(
                "1bf45b237525dfb1c8bdf41ecfceb3b57bcf445755af0f3e62a55c916fd10dd0"
            ),
            alloy_primitives::b256!(
                "25a8ad84b92a2abebb58d4b5d52f4395c35e65bae676dd6e0493f2a40d5b7968"
            ),
        ];
        for tx_hash in usdg {
            let transaction = rpc.transaction_by_hash(tx_hash).await.unwrap().unwrap();
            let receipt = rpc.receipt(tx_hash).await.unwrap().unwrap();
            let block = rpc.block_by_number(receipt.l2_block_number).await.unwrap();
            assert_eq!(
                observe_stonks_v3_direct_launch_at_receipt_block(
                    &rpc,
                    &transaction,
                    &receipt,
                    &block
                )
                .await,
                Err(StonksV3ObserveError::Calldata)
            );
        }
    }
}
