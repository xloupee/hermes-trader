use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use alloy_sol_types::{SolCall, sol};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::noxa_abi::ReceiptLog;
use crate::noxa_predict::{
    DEX_CONFIG_SELECTOR, LAUNCH_CONFIG_SELECTOR, NoxaDexConfig, NoxaLaunchConfig, config_call,
    decode_active_dex_config, decode_dex_config, decode_launch_config,
};
use crate::robinhood::{CHAIN_ID, NOXA_LAUNCH_FACTORY, PUBLIC_RPC_URL};

sol! {
    interface INoxaTokenView {
        function launchFactory() external view returns (address);
        function liquidityPool() external view returns (address);
        function pairToken() external view returns (address);
        function poolFee() external view returns (uint24);
        function maxWalletLimit() external view returns (uint256);
        function maxTxLimit() external view returns (uint256);
        function restrictionEndBlock() external view returns (uint256);
        function balanceOf(address account) external view returns (uint256);
        function allowance(address owner, address spender) external view returns (uint256);
    }

    interface IActiveNoxaTokenView {
        function factory() external view returns (address);
        function maxWalletAmount() external view returns (uint256);
        function maxTxAmount() external view returns (uint256);
        function restrictionsEndBlock() external view returns (uint256);
    }

    struct ActiveLaunchedToken {
        address token;
        address deployer;
        address feeWallet;
        address pairToken;
        address pool;
        uint256 dexId;
        uint256 launchConfigId;
        uint256 positionId;
        uint256 restrictionsEndBlock;
        uint256 initialBuyAmount;
        uint256 createdAtBlock;
        bool isToken0;
    }

    interface IActiveNoxaFactoryView {
        function getLaunchedToken(address token) external view returns (ActiveLaunchedToken memory);
    }
}

const LAUNCH_ENABLED_SELECTOR: &str = "0x236a4afb";
const LAUNCH_FEE_SELECTOR: &str = "0xcf3cf573";
const OWNER_SELECTOR: &str = "0x8da5cb5b";
const TOKEN_LAUNCHED_TOPIC: B256 =
    alloy_primitives::b256!("db51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a");
const V3_SWAP_TOPIC: B256 =
    alloy_primitives::b256!("c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67");
const RPC_ATTEMPTS: usize = 6;
const RPC_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const RPC_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone)]
pub struct NoxaRpcClient {
    client: Client,
    url: String,
    metrics: Arc<RpcMetrics>,
}

#[derive(Default)]
struct RpcMetrics {
    logical_requests: AtomicU64,
    http_attempts: AtomicU64,
    retries: AtomicU64,
    rate_limited: AtomicU64,
    server_errors: AtomicU64,
    transport_errors: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct RpcMetricsSnapshot {
    pub logical_requests: u64,
    pub http_attempts: u64,
    pub retries: u64,
    pub rate_limited: u64,
    pub server_errors: u64,
    pub transport_errors: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FactoryStatus {
    pub chain_id: u64,
    pub pinned_l2_block: u64,
    pub pinned_l1_block: u64,
    pub launch_enabled: bool,
    pub launch_fee: U256,
    pub runtime_bytes: usize,
    pub runtime_keccak256: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RobinhoodBlock {
    pub l2_block_number: u64,
    pub l1_block_number: u64,
    pub timestamp: u64,
    pub hash: B256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RobinhoodTransaction {
    pub hash: B256,
    pub from: Address,
    pub to: Option<Address>,
    pub input: Bytes,
    pub value: U256,
    pub l2_block_number: Option<u64>,
    pub transaction_index: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoxaReceipt {
    pub transaction_hash: B256,
    pub block_hash: B256,
    pub status: bool,
    pub l2_block_number: u64,
    pub l1_block_number: Option<u64>,
    pub transaction_index: u64,
    pub gas_used: Option<u64>,
    pub effective_gas_price: Option<U256>,
    pub logs: Vec<ReceiptLog>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ObservedLaunchLog {
    pub l2_block_number: u64,
    pub block_hash: B256,
    pub transaction_hash: B256,
    pub transaction_index: u64,
    pub log: ReceiptLog,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ObservedPoolSwapLog {
    pub l2_block_number: u64,
    pub transaction_hash: B256,
    pub transaction_index: u64,
    pub log: ReceiptLog,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TokenRestrictionSnapshot {
    pub token: Address,
    pub l2_block_number: u64,
    pub launch_factory: Address,
    pub liquidity_pool: Address,
    pub pair_token: Address,
    pub pool_fee: u32,
    pub max_wallet_limit: U256,
    pub max_tx_limit: U256,
    pub restriction_end_block: U256,
    pub recipient: Option<Address>,
    pub recipient_balance: Option<U256>,
}

/// Active N0xa tokens intentionally expose a smaller token-view ABI than the
/// retired NOXA deployment. Pool identity comes from the audited factory and
/// canonical V3 CREATE2 calculation, never from token-provided metadata.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ActiveNoxaTokenSnapshot {
    pub token: Address,
    pub l2_block_number: u64,
    pub factory: Address,
    pub max_wallet_amount: U256,
    pub max_tx_amount: U256,
    pub restrictions_end_block: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ActiveNoxaLaunchRecord {
    pub token: Address,
    pub deployer: Address,
    pub fee_wallet: Address,
    pub pair_token: Address,
    pub pool: Address,
    pub dex_id: U256,
    pub launch_config_id: U256,
    pub position_id: U256,
    pub restrictions_end_block: U256,
    pub initial_buy_amount: U256,
    pub created_at_block: U256,
    pub is_token0: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3PoolSnapshot {
    pub pool: Address,
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub liquidity: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodCurveStateSnapshot {
    pub virtual_eth: U256,
    pub virtual_tokens: U256,
    pub real_eth: U256,
    pub real_tokens: U256,
    pub creator: Address,
    /// Hood stores the Robinhood L1 block number in this field.
    pub created_at_block: u64,
    pub graduated: bool,
    pub migrated: bool,
    pub trade_fee_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodConfigSnapshot {
    pub virtual_eth_seed: U256,
    pub creation_fee: U256,
    pub default_trade_fee_bps: u16,
    pub migration_fee: U256,
    pub migration_fee_bps: u16,
    pub guard_blocks: u16,
    pub guard_max_wallet_bps: u16,
    pub creator_fee_share_bps: u16,
    pub vanity_enforced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodMarketSnapshot {
    pub factory: Address,
    pub token: Address,
    pub l2_block_number: u64,
    pub curve: HoodCurveStateSnapshot,
    pub config: HoodConfigSnapshot,
    pub token_curve_supply: U256,
    pub token_lp_supply: U256,
    pub migrator: Address,
    pub uniswap_factory: Address,
    pub weth: Address,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct HoodProtocolSnapshot {
    pub factory: Address,
    pub l2_block_number: u64,
    pub config: HoodConfigSnapshot,
    pub migrator: Address,
    pub fallback_factory: Address,
    pub weth: Address,
    pub owner: Address,
    pub pending_owner: Address,
    pub owner_safe_singleton: Address,
    pub migrator_launchpad: Address,
    pub migrator_position_manager: Address,
    pub migrator_locker: Address,
    pub migrator_weth: Address,
    pub migrator_protocol: Address,
    pub migrator_creator_share_bps: u16,
    pub migrator_v3_fee: u32,
    pub locker_position_manager: Address,
    pub locker_weth: Address,
    pub locker_burn_bps: u16,
    pub position_manager_factory: Address,
    pub position_manager_weth: Address,
    pub router_factory: Address,
    pub router_weth: Address,
}

impl NoxaRpcClient {
    pub fn new() -> Result<Self> {
        Self::with_url(PUBLIC_RPC_URL)
    }

    pub fn with_url(url: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .tcp_nodelay(true)
            .http2_adaptive_window(true)
            .pool_idle_timeout(None)
            .connect_timeout(RPC_CONNECT_TIMEOUT)
            .timeout(RPC_REQUEST_TIMEOUT)
            .build()
            .context("build Robinhood read RPC client")?;
        Ok(Self {
            client,
            url: url.into(),
            metrics: Arc::new(RpcMetrics::default()),
        })
    }

    pub fn metrics(&self) -> RpcMetricsSnapshot {
        RpcMetricsSnapshot {
            logical_requests: self.metrics.logical_requests.load(Ordering::Relaxed),
            http_attempts: self.metrics.http_attempts.load(Ordering::Relaxed),
            retries: self.metrics.retries.load(Ordering::Relaxed),
            rate_limited: self.metrics.rate_limited.load(Ordering::Relaxed),
            server_errors: self.metrics.server_errors.load(Ordering::Relaxed),
            transport_errors: self.metrics.transport_errors.load(Ordering::Relaxed),
        }
    }

    pub async fn factory_status(&self) -> Result<FactoryStatus> {
        self.factory_status_for(NOXA_LAUNCH_FACTORY).await
    }

    /// Read the immutable-at-a-block launch status for an explicitly selected,
    /// independently pinned factory. This does not make an arbitrary factory
    /// trusted; callers must separately check its runtime hash and config.
    pub async fn factory_status_for(&self, factory: Address) -> Result<FactoryStatus> {
        let chain_id = self.chain_id().await?;
        if chain_id != CHAIN_ID {
            bail!("RPC chain ID {chain_id} does not match Robinhood {CHAIN_ID}");
        }
        let latest = parse_u64_value(&self.request("eth_blockNumber", json!([])).await?)?;
        let block_tag = hex_u64(latest);
        let block = self.block_by_number(latest).await?;
        let enabled = self
            .eth_call(factory, LAUNCH_ENABLED_SELECTOR, &block_tag)
            .await?;
        let fee = self
            .eth_call(factory, LAUNCH_FEE_SELECTOR, &block_tag)
            .await?;
        let code = parse_bytes_value(
            &self
                .request("eth_getCode", json!([factory, block_tag]))
                .await?,
        )?;
        Ok(FactoryStatus {
            chain_id,
            pinned_l2_block: latest,
            pinned_l1_block: block.l1_block_number,
            launch_enabled: parse_u256_bytes(&enabled)? != U256::ZERO,
            launch_fee: parse_u256_bytes(&fee)?,
            runtime_bytes: code.len(),
            runtime_keccak256: keccak256(&code),
        })
    }

    pub async fn chain_id(&self) -> Result<u64> {
        parse_u64_value(&self.request("eth_chainId", json!([])).await?)
    }

    pub async fn latest_block_number(&self) -> Result<u64> {
        parse_u64_value(&self.request("eth_blockNumber", json!([])).await?)
    }

    pub async fn pending_nonce(&self, account: Address) -> Result<u64> {
        parse_u64_value(
            &self
                .request("eth_getTransactionCount", json!([account, "pending"]))
                .await?,
        )
    }

    pub async fn native_balance(&self, account: Address) -> Result<U256> {
        parse_u256_value(
            &self
                .request("eth_getBalance", json!([account, "latest"]))
                .await?,
        )
    }

    pub async fn code_at(&self, address: Address) -> Result<Bytes> {
        self.code_at_block(address, "latest").await
    }

    pub async fn code_at_l2_block(&self, address: Address, l2_block_number: u64) -> Result<Bytes> {
        self.code_at_block(address, &hex_u64(l2_block_number)).await
    }

    async fn code_at_block(&self, address: Address, block_tag: &str) -> Result<Bytes> {
        parse_bytes_value(
            &self
                .request("eth_getCode", json!([address, block_tag]))
                .await?,
        )
    }

    pub async fn launch_config_at(
        &self,
        id: U256,
        l2_block_number: u64,
    ) -> Result<NoxaLaunchConfig> {
        self.launch_config_at_for(NOXA_LAUNCH_FACTORY, id, l2_block_number)
            .await
    }

    pub async fn launch_config_at_for(
        &self,
        factory: Address,
        id: U256,
        l2_block_number: u64,
    ) -> Result<NoxaLaunchConfig> {
        let call = config_call(LAUNCH_CONFIG_SELECTOR, id);
        let bytes = self
            .eth_call_data(factory, &call, &hex_u64(l2_block_number))
            .await?;
        decode_launch_config(&bytes).map_err(Into::into)
    }

    pub async fn dex_config_at(&self, id: U256, l2_block_number: u64) -> Result<NoxaDexConfig> {
        self.dex_config_at_for(NOXA_LAUNCH_FACTORY, id, l2_block_number)
            .await
    }

    pub async fn dex_config_at_for(
        &self,
        factory: Address,
        id: U256,
        l2_block_number: u64,
    ) -> Result<NoxaDexConfig> {
        let call = config_call(DEX_CONFIG_SELECTOR, id);
        let bytes = self
            .eth_call_data(factory, &call, &hex_u64(l2_block_number))
            .await?;
        decode_dex_config(&bytes).map_err(Into::into)
    }

    pub async fn active_dex_config_at_for(
        &self,
        factory: Address,
        id: U256,
        l2_block_number: u64,
    ) -> Result<NoxaDexConfig> {
        let call = config_call(DEX_CONFIG_SELECTOR, id);
        let bytes = self
            .eth_call_data(factory, &call, &hex_u64(l2_block_number))
            .await?;
        decode_active_dex_config(&bytes).map_err(Into::into)
    }

    pub async fn erc20_balance(&self, token: Address, account: Address) -> Result<U256> {
        let call = INoxaTokenView::balanceOfCall { account }.abi_encode();
        let bytes = self.eth_call_data(token, &call, "latest").await?;
        parse_u256_bytes(&bytes)
    }

    pub async fn erc20_allowance(
        &self,
        token: Address,
        owner: Address,
        spender: Address,
    ) -> Result<U256> {
        let call = INoxaTokenView::allowanceCall { owner, spender }.abi_encode();
        let bytes = self.eth_call_data(token, &call, "latest").await?;
        parse_u256_bytes(&bytes)
    }

    pub async fn v3_pool_snapshot(&self, pool: Address) -> Result<V3PoolSnapshot> {
        self.v3_pool_snapshot_at_tag(pool, "latest").await
    }

    pub async fn v3_pool_snapshot_at(
        &self,
        pool: Address,
        l2_block_number: u64,
    ) -> Result<V3PoolSnapshot> {
        self.v3_pool_snapshot_at_tag(pool, &hex_u64(l2_block_number))
            .await
    }

    /// Hydrate the exact Hood curve and semantic configuration at a confirmed
    /// L2 block. This is receipt-side evidence collection only; it is never
    /// used on the candidate decision path.
    pub async fn hood_market_snapshot_at(
        &self,
        factory: Address,
        token: Address,
        l2_block_number: u64,
    ) -> Result<HoodMarketSnapshot> {
        let block_tag = hex_u64(l2_block_number);
        let address_call = |signature: &str| {
            let digest = keccak256(signature.as_bytes());
            let mut call = Vec::with_capacity(36);
            call.extend_from_slice(&digest[..4]);
            call.extend_from_slice(&[0_u8; 12]);
            call.extend_from_slice(token.as_slice());
            call
        };
        let selector_call = |signature: &str| keccak256(signature.as_bytes())[..4].to_vec();
        let curves_call = address_call("curves(address)");
        let curve_supply_call = address_call("tokenCurveSupply(address)");
        let lp_supply_call = address_call("tokenLpSupply(address)");
        let config_call = selector_call("config()");
        let migrator_call = selector_call("migrator()");
        let factory_call = selector_call("uniswapFactory()");
        let weth_call = selector_call("weth()");
        let (curve, curve_supply, lp_supply, config, migrator, uniswap_factory, weth) = tokio::try_join!(
            self.eth_call_data(factory, &curves_call, &block_tag),
            self.eth_call_data(factory, &curve_supply_call, &block_tag),
            self.eth_call_data(factory, &lp_supply_call, &block_tag),
            self.eth_call_data(factory, &config_call, &block_tag),
            self.eth_call_data(factory, &migrator_call, &block_tag),
            self.eth_call_data(factory, &factory_call, &block_tag),
            self.eth_call_data(factory, &weth_call, &block_tag),
        )?;
        let curve_words = fixed_words(&curve, 9, "Hood curves(address)")?;
        let config_words = fixed_words(&config, 9, "Hood config()")?;
        Ok(HoodMarketSnapshot {
            factory,
            token,
            l2_block_number,
            curve: HoodCurveStateSnapshot {
                virtual_eth: curve_words[0],
                virtual_tokens: curve_words[1],
                real_eth: curve_words[2],
                real_tokens: curve_words[3],
                creator: address_from_word(curve_words[4], "Hood creator")?,
                created_at_block: u64::try_from(curve_words[5])
                    .context("Hood createdAtBlock does not fit u64")?,
                graduated: bool_from_word(curve_words[6], "Hood graduated")?,
                migrated: bool_from_word(curve_words[7], "Hood migrated")?,
                trade_fee_bps: u16::try_from(curve_words[8])
                    .context("Hood tradeFeeBps does not fit u16")?,
            },
            config: HoodConfigSnapshot {
                virtual_eth_seed: config_words[0],
                creation_fee: config_words[1],
                default_trade_fee_bps: u16::try_from(config_words[2])
                    .context("Hood defaultTradeFeeBps does not fit u16")?,
                migration_fee: config_words[3],
                migration_fee_bps: u16::try_from(config_words[4])
                    .context("Hood migrationFeeBps does not fit u16")?,
                guard_blocks: u16::try_from(config_words[5])
                    .context("Hood guardBlocks does not fit u16")?,
                guard_max_wallet_bps: u16::try_from(config_words[6])
                    .context("Hood guardMaxWalletBps does not fit u16")?,
                creator_fee_share_bps: u16::try_from(config_words[7])
                    .context("Hood creatorFeeShareBps does not fit u16")?,
                vanity_enforced: bool_from_word(config_words[8], "Hood vanityEnforced")?,
            },
            token_curve_supply: parse_u256_bytes(&curve_supply)?,
            token_lp_supply: parse_u256_bytes(&lp_supply)?,
            migrator: parse_address_word(&migrator)?,
            uniswap_factory: parse_address_word(&uniswap_factory)?,
            weth: parse_address_word(&weth)?,
        })
    }

    /// Capture Hood's mutable configuration and every cross-contract link used
    /// by the curve-to-V3 path at one startup block. Runtime bytes are captured
    /// separately by the pin snapshot; both layers must agree.
    #[allow(clippy::too_many_arguments)]
    pub async fn hood_protocol_snapshot_at(
        &self,
        factory: Address,
        migrator: Address,
        locker: Address,
        position_manager: Address,
        router: Address,
        owner_safe: Address,
        l2_block_number: u64,
    ) -> Result<HoodProtocolSnapshot> {
        let block_tag = hex_u64(l2_block_number);
        let selector = |signature: &str| keccak256(signature.as_bytes())[..4].to_vec();
        let selectors = [
            selector("config()"),
            selector("migrator()"),
            selector("uniswapFactory()"),
            selector("weth()"),
            selector("owner()"),
            selector("pendingOwner()"),
            selector("launchpad()"),
            selector("nfpm()"),
            selector("locker()"),
            selector("protocol()"),
            selector("creatorShareBps()"),
            selector("FEE()"),
            selector("WETH()"),
            selector("BURN_BPS()"),
            selector("factory()"),
            selector("WETH9()"),
        ];
        let calls = tokio::try_join!(
            self.eth_call_data(factory, &selectors[0], &block_tag),
            self.eth_call_data(factory, &selectors[1], &block_tag),
            self.eth_call_data(factory, &selectors[2], &block_tag),
            self.eth_call_data(factory, &selectors[3], &block_tag),
            self.eth_call_data(factory, &selectors[4], &block_tag),
            self.eth_call_data(factory, &selectors[5], &block_tag),
            self.eth_call_data(migrator, &selectors[6], &block_tag),
            self.eth_call_data(migrator, &selectors[7], &block_tag),
            self.eth_call_data(migrator, &selectors[8], &block_tag),
            self.eth_call_data(migrator, &selectors[3], &block_tag),
            self.eth_call_data(migrator, &selectors[9], &block_tag),
            self.eth_call_data(migrator, &selectors[10], &block_tag),
            self.eth_call_data(migrator, &selectors[11], &block_tag),
            self.eth_call_data(locker, &selectors[7], &block_tag),
            self.eth_call_data(locker, &selectors[12], &block_tag),
            self.eth_call_data(locker, &selectors[13], &block_tag),
            self.eth_call_data(position_manager, &selectors[14], &block_tag),
            self.eth_call_data(position_manager, &selectors[15], &block_tag),
            self.eth_call_data(router, &selectors[14], &block_tag),
            self.eth_call_data(router, &selectors[15], &block_tag),
        )?;
        let config_words = fixed_words(&calls.0, 9, "Hood config()")?;
        let storage = parse_bytes_value(
            &self
                .request("eth_getStorageAt", json!([owner_safe, "0x0", block_tag]))
                .await?,
        )?;
        Ok(HoodProtocolSnapshot {
            factory,
            l2_block_number,
            config: HoodConfigSnapshot {
                virtual_eth_seed: config_words[0],
                creation_fee: config_words[1],
                default_trade_fee_bps: u16::try_from(config_words[2])?,
                migration_fee: config_words[3],
                migration_fee_bps: u16::try_from(config_words[4])?,
                guard_blocks: u16::try_from(config_words[5])?,
                guard_max_wallet_bps: u16::try_from(config_words[6])?,
                creator_fee_share_bps: u16::try_from(config_words[7])?,
                vanity_enforced: bool_from_word(config_words[8], "Hood vanityEnforced")?,
            },
            migrator: parse_address_word(&calls.1)?,
            fallback_factory: parse_address_word(&calls.2)?,
            weth: parse_address_word(&calls.3)?,
            owner: parse_address_word(&calls.4)?,
            pending_owner: parse_address_word(&calls.5)?,
            owner_safe_singleton: parse_address_word(&storage)?,
            migrator_launchpad: parse_address_word(&calls.6)?,
            migrator_position_manager: parse_address_word(&calls.7)?,
            migrator_locker: parse_address_word(&calls.8)?,
            migrator_weth: parse_address_word(&calls.9)?,
            migrator_protocol: parse_address_word(&calls.10)?,
            migrator_creator_share_bps: u16::try_from(parse_u256_bytes(&calls.11)?)?,
            migrator_v3_fee: u32::try_from(parse_u256_bytes(&calls.12)?)?,
            locker_position_manager: parse_address_word(&calls.13)?,
            locker_weth: parse_address_word(&calls.14)?,
            locker_burn_bps: u16::try_from(parse_u256_bytes(&calls.15)?)?,
            position_manager_factory: parse_address_word(&calls.16)?,
            position_manager_weth: parse_address_word(&calls.17)?,
            router_factory: parse_address_word(&calls.18)?,
            router_weth: parse_address_word(&calls.19)?,
        })
    }

    async fn v3_pool_snapshot_at_tag(
        &self,
        pool: Address,
        block_tag: &str,
    ) -> Result<V3PoolSnapshot> {
        const TOKEN0: [u8; 4] = [0x0d, 0xfe, 0x16, 0x81];
        const TOKEN1: [u8; 4] = [0xd2, 0x12, 0x20, 0xa7];
        const FEE: [u8; 4] = [0xdd, 0xca, 0x3f, 0x43];
        const LIQUIDITY: [u8; 4] = [0x1a, 0x68, 0x65, 0x02];
        let (token0, token1, fee, liquidity) = tokio::try_join!(
            self.eth_call_data(pool, &TOKEN0, block_tag),
            self.eth_call_data(pool, &TOKEN1, block_tag),
            self.eth_call_data(pool, &FEE, block_tag),
            self.eth_call_data(pool, &LIQUIDITY, block_tag),
        )?;
        Ok(V3PoolSnapshot {
            pool,
            token0: parse_address_word(&token0)?,
            token1: parse_address_word(&token1)?,
            fee: parse_u32_word(&fee)?,
            liquidity: u128::try_from(parse_u256_bytes(&liquidity)?)
                .context("V3 liquidity word does not fit u128")?,
        })
    }

    pub async fn block_by_number(&self, l2_block_number: u64) -> Result<RobinhoodBlock> {
        let value = self
            .request(
                "eth_getBlockByNumber",
                json!([hex_u64(l2_block_number), false]),
            )
            .await?;
        if value.is_null() {
            bail!("missing Robinhood block {l2_block_number}");
        }
        parse_block(&value)
    }

    pub async fn latest_block(&self) -> Result<RobinhoodBlock> {
        let value = self
            .request("eth_getBlockByNumber", json!(["latest", false]))
            .await?;
        if value.is_null() {
            bail!("Robinhood RPC returned no latest block");
        }
        parse_block(&value)
    }

    /// Returns the current EIP-1559 base fee. Signed canaries use this to
    /// reject a fee envelope that is already stale before any bytes are signed.
    pub async fn latest_base_fee_per_gas(&self) -> Result<u128> {
        let value = self
            .request("eth_getBlockByNumber", json!(["latest", false]))
            .await?;
        parse_base_fee_per_gas(&value)
    }

    pub async fn transaction_by_hash(&self, hash: B256) -> Result<Option<RobinhoodTransaction>> {
        let value = self
            .request("eth_getTransactionByHash", json!([hash]))
            .await?;
        if value.is_null() {
            return Ok(None);
        }
        Ok(Some(parse_transaction(&value)?))
    }

    pub async fn receipt(&self, hash: B256) -> Result<Option<NoxaReceipt>> {
        let value = self
            .request("eth_getTransactionReceipt", json!([hash]))
            .await?;
        if value.is_null() {
            return Ok(None);
        }
        Ok(Some(parse_receipt(&value)?))
    }

    pub async fn launch_fee_at(&self, l2_block_number: u64) -> Result<U256> {
        self.launch_fee_at_for(NOXA_LAUNCH_FACTORY, l2_block_number)
            .await
    }

    pub async fn launch_fee_at_for(&self, factory: Address, l2_block_number: u64) -> Result<U256> {
        let bytes = self
            .eth_call(factory, LAUNCH_FEE_SELECTOR, &hex_u64(l2_block_number))
            .await?;
        parse_u256_bytes(&bytes)
    }

    pub async fn factory_owner_at(&self, l2_block_number: u64) -> Result<Address> {
        self.factory_owner_at_for(NOXA_LAUNCH_FACTORY, l2_block_number)
            .await
    }

    pub async fn factory_owner_at_for(
        &self,
        factory: Address,
        l2_block_number: u64,
    ) -> Result<Address> {
        let bytes = self
            .eth_call(factory, OWNER_SELECTOR, &hex_u64(l2_block_number))
            .await?;
        parse_address_word(&bytes)
    }

    pub async fn token_restriction_snapshot(
        &self,
        token: Address,
        l2_block_number: u64,
        recipient: Option<Address>,
    ) -> Result<TokenRestrictionSnapshot> {
        let block_tag = hex_u64(l2_block_number);
        let launch_factory_call = INoxaTokenView::launchFactoryCall {}.abi_encode();
        let liquidity_pool_call = INoxaTokenView::liquidityPoolCall {}.abi_encode();
        let pair_token_call = INoxaTokenView::pairTokenCall {}.abi_encode();
        let pool_fee_call = INoxaTokenView::poolFeeCall {}.abi_encode();
        let max_wallet_limit_call = INoxaTokenView::maxWalletLimitCall {}.abi_encode();
        let max_tx_limit_call = INoxaTokenView::maxTxLimitCall {}.abi_encode();
        let restriction_end_block_call = INoxaTokenView::restrictionEndBlockCall {}.abi_encode();
        let balance_call =
            recipient.map(|account| INoxaTokenView::balanceOfCall { account }.abi_encode());
        let balance_request = async {
            match balance_call.as_deref() {
                Some(call) => self.eth_call_data(token, call, &block_tag).await.map(Some),
                None => Ok(None),
            }
        };
        let (
            launch_factory,
            liquidity_pool,
            pair_token,
            pool_fee,
            max_wallet_limit,
            max_tx_limit,
            restriction_end_block,
            recipient_balance,
        ) = tokio::try_join!(
            self.eth_call_data(token, &launch_factory_call, &block_tag),
            self.eth_call_data(token, &liquidity_pool_call, &block_tag),
            self.eth_call_data(token, &pair_token_call, &block_tag),
            self.eth_call_data(token, &pool_fee_call, &block_tag),
            self.eth_call_data(token, &max_wallet_limit_call, &block_tag),
            self.eth_call_data(token, &max_tx_limit_call, &block_tag),
            self.eth_call_data(token, &restriction_end_block_call, &block_tag),
            balance_request,
        )?;
        Ok(TokenRestrictionSnapshot {
            token,
            l2_block_number,
            launch_factory: parse_address_word(&launch_factory)?,
            liquidity_pool: parse_address_word(&liquidity_pool)?,
            pair_token: parse_address_word(&pair_token)?,
            pool_fee: parse_u32_word(&pool_fee)?,
            max_wallet_limit: parse_u256_bytes(&max_wallet_limit)?,
            max_tx_limit: parse_u256_bytes(&max_tx_limit)?,
            restriction_end_block: parse_u256_bytes(&restriction_end_block)?,
            recipient,
            recipient_balance: recipient_balance
                .as_ref()
                .map(|bytes| parse_u256_bytes(bytes.as_ref()))
                .transpose()?,
        })
    }

    pub async fn active_noxa_token_snapshot(
        &self,
        token: Address,
        l2_block_number: u64,
    ) -> Result<ActiveNoxaTokenSnapshot> {
        let block_tag = hex_u64(l2_block_number);
        let factory_call = IActiveNoxaTokenView::factoryCall {}.abi_encode();
        let max_wallet_call = IActiveNoxaTokenView::maxWalletAmountCall {}.abi_encode();
        let max_tx_call = IActiveNoxaTokenView::maxTxAmountCall {}.abi_encode();
        let restrictions_call = IActiveNoxaTokenView::restrictionsEndBlockCall {}.abi_encode();
        let (factory, max_wallet_amount, max_tx_amount, restrictions_end_block) = tokio::try_join!(
            self.eth_call_data(token, &factory_call, &block_tag),
            self.eth_call_data(token, &max_wallet_call, &block_tag),
            self.eth_call_data(token, &max_tx_call, &block_tag),
            self.eth_call_data(token, &restrictions_call, &block_tag),
        )?;
        Ok(ActiveNoxaTokenSnapshot {
            token,
            l2_block_number,
            factory: parse_address_word(&factory)?,
            max_wallet_amount: parse_u256_bytes(&max_wallet_amount)?,
            max_tx_amount: parse_u256_bytes(&max_tx_amount)?,
            restrictions_end_block: parse_u256_bytes(&restrictions_end_block)?,
        })
    }

    pub async fn active_noxa_launch_record(
        &self,
        factory: Address,
        token: Address,
        l2_block_number: u64,
    ) -> Result<ActiveNoxaLaunchRecord> {
        let call = IActiveNoxaFactoryView::getLaunchedTokenCall { token }.abi_encode();
        let bytes = self
            .eth_call_data(factory, &call, &hex_u64(l2_block_number))
            .await?;
        decode_active_noxa_launch_record(&bytes)
    }

    pub async fn token_launched_logs(
        &self,
        from_l2_block: u64,
        to_l2_block: u64,
    ) -> Result<Vec<ObservedLaunchLog>> {
        self.token_launched_logs_for(NOXA_LAUNCH_FACTORY, from_l2_block, to_l2_block)
            .await
    }

    pub async fn token_launched_logs_for(
        &self,
        factory: Address,
        from_l2_block: u64,
        to_l2_block: u64,
    ) -> Result<Vec<ObservedLaunchLog>> {
        if from_l2_block > to_l2_block {
            bail!("log range start exceeds end");
        }
        let value = self
            .request(
                "eth_getLogs",
                json!([{
                    "fromBlock": hex_u64(from_l2_block),
                    "toBlock": hex_u64(to_l2_block),
                    "address": factory,
                    "topics": [TOKEN_LAUNCHED_TOPIC],
                }]),
            )
            .await?;
        let logs = value
            .as_array()
            .ok_or_else(|| anyhow!("eth_getLogs result is not an array"))?;
        logs.iter().map(parse_observed_launch_log).collect()
    }

    pub async fn v3_swap_logs(
        &self,
        pools: &[Address],
        from_l2_block: u64,
        to_l2_block: u64,
    ) -> Result<Vec<ObservedPoolSwapLog>> {
        if pools.is_empty() {
            return Ok(Vec::new());
        }
        if from_l2_block > to_l2_block {
            bail!("log range start exceeds end");
        }
        let value = self
            .request(
                "eth_getLogs",
                json!([{
                    "fromBlock": hex_u64(from_l2_block),
                    "toBlock": hex_u64(to_l2_block),
                    "address": pools,
                    "topics": [V3_SWAP_TOPIC],
                }]),
            )
            .await?;
        let logs = value
            .as_array()
            .ok_or_else(|| anyhow!("eth_getLogs result is not an array"))?;
        logs.iter().map(parse_observed_pool_swap_log).collect()
    }

    /// Fetch a bounded, independently selected protocol-event set. Callers
    /// remain responsible for exact address/topic pairing; the RPC filter uses
    /// OR semantics only to avoid one request per reviewed launchpad.
    pub async fn protocol_event_logs(
        &self,
        addresses: &[Address],
        topics: &[B256],
        from_l2_block: u64,
        to_l2_block: u64,
    ) -> Result<Vec<ObservedLaunchLog>> {
        if addresses.is_empty() || topics.is_empty() {
            return Ok(Vec::new());
        }
        if from_l2_block > to_l2_block {
            bail!("protocol-event log range start exceeds end");
        }
        let value = self
            .request(
                "eth_getLogs",
                json!([{
                    "fromBlock": hex_u64(from_l2_block),
                    "toBlock": hex_u64(to_l2_block),
                    "address": addresses,
                    "topics": [topics],
                }]),
            )
            .await?;
        let logs = value
            .as_array()
            .ok_or_else(|| anyhow!("protocol eth_getLogs result is not an array"))?;
        logs.iter().map(parse_observed_launch_log).collect()
    }

    async fn eth_call(&self, to: Address, data: &str, block_tag: &str) -> Result<Bytes> {
        let value = self
            .request("eth_call", json!([{"to": to, "data": data}, block_tag]))
            .await?;
        parse_bytes_value(&value)
    }

    async fn eth_call_data(&self, to: Address, data: &[u8], block_tag: &str) -> Result<Bytes> {
        let value = self
            .request(
                "eth_call",
                json!([{"to": to, "data": format!("0x{}", hex::encode(data))}, block_tag]),
            )
            .await?;
        parse_bytes_value(&value)
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.metrics
            .logical_requests
            .fetch_add(1, Ordering::Relaxed);
        let mut backoff = std::time::Duration::from_millis(100);
        for attempt in 0..RPC_ATTEMPTS {
            self.metrics.http_attempts.fetch_add(1, Ordering::Relaxed);
            let response = match self
                .client
                .post(&self.url)
                .json(&json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": method,
                    "params": &params,
                }))
                .send()
                .await
            {
                Ok(response) => response,
                Err(_) if attempt + 1 < RPC_ATTEMPTS => {
                    self.metrics
                        .transport_errors
                        .fetch_add(1, Ordering::Relaxed);
                    self.metrics.retries.fetch_add(1, Ordering::Relaxed);
                    sleep_before_retry(&mut backoff).await;
                    continue;
                }
                Err(error) => {
                    self.metrics
                        .transport_errors
                        .fetch_add(1, Ordering::Relaxed);
                    return Err(error)
                        .with_context(|| format!("send {method} to Robinhood RPC after retries"));
                }
            };
            let status = response.status();
            let bytes = match response.bytes().await {
                Ok(bytes) => bytes,
                Err(_) if attempt + 1 < RPC_ATTEMPTS => {
                    self.metrics
                        .transport_errors
                        .fetch_add(1, Ordering::Relaxed);
                    self.metrics.retries.fetch_add(1, Ordering::Relaxed);
                    sleep_before_retry(&mut backoff).await;
                    continue;
                }
                Err(error) => {
                    self.metrics
                        .transport_errors
                        .fetch_add(1, Ordering::Relaxed);
                    return Err(error)
                        .with_context(|| format!("read {method} RPC body after retries"));
                }
            };
            match classify_rpc_response(status, &bytes) {
                RpcResponse::Result(value) => return Ok(value),
                RpcResponse::Retryable(_) if attempt + 1 < RPC_ATTEMPTS => {
                    if response_is_rate_limited(status, &bytes) {
                        self.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
                    }
                    if status.is_server_error() {
                        self.metrics.server_errors.fetch_add(1, Ordering::Relaxed);
                    }
                    self.metrics.retries.fetch_add(1, Ordering::Relaxed);
                    sleep_before_retry(&mut backoff).await;
                }
                RpcResponse::Retryable(reason) => {
                    if response_is_rate_limited(status, &bytes) {
                        self.metrics.rate_limited.fetch_add(1, Ordering::Relaxed);
                    }
                    if status.is_server_error() {
                        self.metrics.server_errors.fetch_add(1, Ordering::Relaxed);
                    }
                    bail!("{method} remained retryable after {RPC_ATTEMPTS} attempts: {reason}");
                }
                RpcResponse::Fatal(reason) => bail!("{method} {reason}"),
            }
        }
        unreachable!("bounded RPC retry loop always returns")
    }
}

fn response_is_rate_limited(status: StatusCode, bytes: &[u8]) -> bool {
    if status == StatusCode::TOO_MANY_REQUESTS {
        return true;
    }
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| value.pointer("/error/code").and_then(Value::as_i64))
        .is_some_and(|code| code == 429 || code == -32_005)
}

#[derive(Debug, PartialEq, Eq)]
enum RpcResponse {
    Result(Value),
    Retryable(String),
    Fatal(String),
}

fn classify_rpc_response(status: StatusCode, bytes: &[u8]) -> RpcResponse {
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return RpcResponse::Retryable(format!(
            "HTTP {status}: {}",
            String::from_utf8_lossy(bytes)
        ));
    }
    if !status.is_success() {
        return RpcResponse::Fatal(format!("HTTP {status}: {}", String::from_utf8_lossy(bytes)));
    }
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(error) => return RpcResponse::Retryable(format!("invalid JSON response: {error}")),
    };
    let rpc_rate_limited = value
        .pointer("/error/code")
        .and_then(Value::as_i64)
        .is_some_and(|code| code == 429 || code == -32_005);
    if rpc_rate_limited {
        return RpcResponse::Retryable(format!("JSON-RPC rate limit: {value}"));
    }
    if let Some(error) = value.get("error") {
        return RpcResponse::Fatal(format!("JSON-RPC error: {error}"));
    }
    match value.get("result") {
        Some(result) => RpcResponse::Result(result.clone()),
        None => RpcResponse::Fatal("response omitted result".into()),
    }
}

async fn sleep_before_retry(backoff: &mut std::time::Duration) {
    tokio::time::sleep(*backoff).await;
    *backoff = (*backoff * 2).min(std::time::Duration::from_secs(2));
}

fn parse_block(value: &Value) -> Result<RobinhoodBlock> {
    Ok(RobinhoodBlock {
        l2_block_number: parse_hex_u64(field_str(value, "number")?)?,
        l1_block_number: parse_hex_u64(field_str(value, "l1BlockNumber")?)?,
        timestamp: parse_hex_u64(field_str(value, "timestamp")?)?,
        hash: field_str(value, "hash")?
            .parse()
            .context("parse block hash")?,
    })
}

fn parse_transaction(value: &Value) -> Result<RobinhoodTransaction> {
    Ok(RobinhoodTransaction {
        hash: field_str(value, "hash")?.parse().context("parse tx hash")?,
        from: field_str(value, "from")?
            .parse()
            .context("parse tx sender")?,
        to: value
            .get("to")
            .and_then(Value::as_str)
            .map(str::parse)
            .transpose()
            .context("parse tx destination")?,
        input: parse_bytes_str(field_str(value, "input")?)?,
        value: parse_hex_u256(field_str(value, "value")?)?,
        l2_block_number: optional_hex_u64(value, "blockNumber")?,
        transaction_index: optional_hex_u64(value, "transactionIndex")?,
    })
}

fn parse_receipt(value: &Value) -> Result<NoxaReceipt> {
    let raw_logs = value
        .get("logs")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("receipt logs missing"))?;
    let logs = raw_logs
        .iter()
        .map(parse_receipt_log)
        .collect::<Result<_>>()?;
    Ok(NoxaReceipt {
        transaction_hash: field_str(value, "transactionHash")?
            .parse()
            .context("parse receipt tx hash")?,
        block_hash: field_str(value, "blockHash")?
            .parse()
            .context("parse receipt block hash")?,
        status: parse_hex_u64(field_str(value, "status")?)? == 1,
        l2_block_number: parse_hex_u64(field_str(value, "blockNumber")?)?,
        l1_block_number: optional_hex_u64(value, "l1BlockNumber")?,
        transaction_index: parse_hex_u64(field_str(value, "transactionIndex")?)?,
        gas_used: optional_hex_u64(value, "gasUsed")?,
        effective_gas_price: value
            .get("effectiveGasPrice")
            .and_then(Value::as_str)
            .map(parse_hex_u256)
            .transpose()?,
        logs,
    })
}

fn parse_receipt_log(value: &Value) -> Result<ReceiptLog> {
    let topics = value
        .get("topics")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("receipt log topics missing"))?
        .iter()
        .map(|topic| {
            topic
                .as_str()
                .ok_or_else(|| anyhow!("receipt topic is not a string"))?
                .parse()
                .context("parse receipt topic")
        })
        .collect::<Result<_>>()?;
    Ok(ReceiptLog {
        address: field_str(value, "address")?
            .parse()
            .context("parse log address")?,
        log_index: parse_hex_u64(field_str(value, "logIndex")?)?,
        topics,
        data: parse_bytes_str(field_str(value, "data")?)?,
    })
}

fn parse_observed_launch_log(value: &Value) -> Result<ObservedLaunchLog> {
    Ok(ObservedLaunchLog {
        l2_block_number: parse_hex_u64(field_str(value, "blockNumber")?)?,
        block_hash: field_str(value, "blockHash")?
            .parse()
            .context("parse log block hash")?,
        transaction_hash: field_str(value, "transactionHash")?
            .parse()
            .context("parse log transaction hash")?,
        transaction_index: parse_hex_u64(field_str(value, "transactionIndex")?)?,
        log: parse_receipt_log(value)?,
    })
}

fn parse_observed_pool_swap_log(value: &Value) -> Result<ObservedPoolSwapLog> {
    Ok(ObservedPoolSwapLog {
        l2_block_number: parse_hex_u64(field_str(value, "blockNumber")?)?,
        transaction_hash: field_str(value, "transactionHash")?
            .parse()
            .context("parse log transaction hash")?,
        transaction_index: parse_hex_u64(field_str(value, "transactionIndex")?)?,
        log: parse_receipt_log(value)?,
    })
}

fn field_str<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing string field {field}"))
}

fn optional_hex_u64(value: &Value, field: &str) -> Result<Option<u64>> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(parse_hex_u64)
        .transpose()
}

fn parse_u64_value(value: &Value) -> Result<u64> {
    parse_hex_u64(
        value
            .as_str()
            .ok_or_else(|| anyhow!("hex quantity is not a string"))?,
    )
}

fn parse_u256_value(value: &Value) -> Result<U256> {
    parse_hex_u256(
        value
            .as_str()
            .ok_or_else(|| anyhow!("hex quantity is not a string"))?,
    )
}

fn parse_hex_u64(value: &str) -> Result<u64> {
    u64::from_str_radix(
        value
            .strip_prefix("0x")
            .ok_or_else(|| anyhow!("hex quantity lacks 0x prefix"))?,
        16,
    )
    .context("parse u64 hex quantity")
}

fn parse_hex_u256(value: &str) -> Result<U256> {
    U256::from_str_radix(
        value
            .strip_prefix("0x")
            .ok_or_else(|| anyhow!("hex quantity lacks 0x prefix"))?,
        16,
    )
    .context("parse U256 hex quantity")
}

fn parse_base_fee_per_gas(value: &Value) -> Result<u128> {
    let base_fee = parse_hex_u256(field_str(value, "baseFeePerGas")?)?;
    u128::try_from(base_fee).context("base fee per gas does not fit u128")
}

fn parse_bytes_value(value: &Value) -> Result<Bytes> {
    parse_bytes_str(
        value
            .as_str()
            .ok_or_else(|| anyhow!("hex bytes result is not a string"))?,
    )
}

fn parse_bytes_str(value: &str) -> Result<Bytes> {
    let hex = value
        .strip_prefix("0x")
        .ok_or_else(|| anyhow!("hex bytes lack 0x prefix"))?;
    Ok(Bytes::from(hex::decode(hex).context("decode hex bytes")?))
}

fn parse_u256_bytes(value: &[u8]) -> Result<U256> {
    if value.len() != 32 {
        bail!("expected one ABI word, got {} bytes", value.len());
    }
    Ok(U256::from_be_slice(value))
}

fn parse_address_word(value: &[u8]) -> Result<Address> {
    if value.len() != 32 {
        bail!("expected one ABI address word, got {} bytes", value.len());
    }
    if value[..12].iter().any(|byte| *byte != 0) {
        bail!("ABI address word has non-zero padding");
    }
    Ok(Address::from_slice(&value[12..]))
}

fn parse_u32_word(value: &[u8]) -> Result<u32> {
    let value = parse_u256_bytes(value)?;
    u32::try_from(value).context("ABI word does not fit u32")
}

fn fixed_words(value: &[u8], count: usize, label: &str) -> Result<Vec<U256>> {
    let expected = count
        .checked_mul(32)
        .ok_or_else(|| anyhow!("{label} ABI length overflow"))?;
    if value.len() != expected {
        bail!(
            "{label} returned {} bytes, expected {expected}",
            value.len()
        );
    }
    Ok(value.chunks_exact(32).map(U256::from_be_slice).collect())
}

fn address_from_word(value: U256, label: &str) -> Result<Address> {
    if value >> 160 != U256::ZERO {
        bail!("{label} ABI address word has non-zero padding");
    }
    let bytes = value.to_be_bytes::<32>();
    Ok(Address::from_slice(&bytes[12..]))
}

fn bool_from_word(value: U256, label: &str) -> Result<bool> {
    match value {
        value if value == U256::ZERO => Ok(false),
        value if value == U256::from(1_u8) => Ok(true),
        _ => bail!("{label} ABI value is not a canonical bool"),
    }
}

fn decode_active_noxa_launch_record(bytes: &[u8]) -> Result<ActiveNoxaLaunchRecord> {
    if bytes.len() != 12 * 32 {
        bail!(
            "active N0xa getLaunchedToken returned {} bytes, expected 384",
            bytes.len()
        );
    }
    let word = |index: usize| -> &[u8] { &bytes[index * 32..(index + 1) * 32] };
    let is_token0 = match parse_u256_bytes(word(11))? {
        value if value == U256::ZERO => false,
        value if value == U256::from(1) => true,
        _ => bail!("active N0xa isToken0 is not ABI bool"),
    };
    Ok(ActiveNoxaLaunchRecord {
        token: parse_address_word(word(0))?,
        deployer: parse_address_word(word(1))?,
        fee_wallet: parse_address_word(word(2))?,
        pair_token: parse_address_word(word(3))?,
        pool: parse_address_word(word(4))?,
        dex_id: parse_u256_bytes(word(5))?,
        launch_config_id: parse_u256_bytes(word(6))?,
        position_id: parse_u256_bytes(word(7))?,
        restrictions_end_block: parse_u256_bytes(word(8))?,
        initial_buy_amount: parse_u256_bytes(word(9))?,
        created_at_block: parse_u256_bytes(word(10))?,
        is_token0,
    })
}

fn hex_u64(value: u64) -> String {
    format!("0x{value:x}")
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{address, b256};

    use super::*;

    #[test]
    fn parses_robinhood_l1_block_field() {
        let value = json!({
            "number": "0x68fd86",
            "l1BlockNumber": "0x1853bf3",
            "timestamp": "0x6a521e91",
            "hash": "0x1111111111111111111111111111111111111111111111111111111111111111"
        });
        let block = parse_block(&value).unwrap();
        assert_eq!(block.l2_block_number, 6_880_646);
        assert_eq!(block.l1_block_number, 25_508_851);
    }

    #[test]
    fn parses_latest_base_fee_and_rejects_missing_or_oversized_values() {
        assert_eq!(
            parse_base_fee_per_gas(&json!({"baseFeePerGas": "0x4082a60"})).unwrap(),
            67_644_000
        );
        assert!(parse_base_fee_per_gas(&json!({})).is_err());
        assert!(
            parse_base_fee_per_gas(
                &json!({"baseFeePerGas": "0x100000000000000000000000000000000"})
            )
            .is_err()
        );
    }

    #[test]
    fn parses_token_launched_rpc_log() {
        let value = json!({
            "address": NOXA_LAUNCH_FACTORY,
            "blockNumber": "0x68fd86",
            "blockHash": B256::with_last_byte(0x86),
            "transactionHash": "0xc62997c2607d579233b552fad71faae7e392a4c13bc92b9d20c57425b9ffe418",
            "transactionIndex": "0x11",
            "logIndex": "0x20",
            "topics": [TOKEN_LAUNCHED_TOPIC, b256!("000000000000000000000000955b339944cbd4834156366d766c260c80956b44")],
            "data": "0x"
        });
        let parsed = parse_observed_launch_log(&value).unwrap();
        assert_eq!(parsed.l2_block_number, 6_880_646);
        assert_eq!(parsed.block_hash, B256::with_last_byte(0x86));
        assert_eq!(parsed.transaction_index, 17);
        assert_eq!(parsed.log.log_index, 32);
        assert_eq!(
            parsed.log.address,
            address!("d9ec2db5f3d1b236843925949fe5bd8a3836fccb")
        );
    }

    #[test]
    fn parses_v3_swap_rpc_log_identity() {
        let pool = address!("94787c100973b364f9419c07764da81d49002610");
        let hash = b256!("021f73d6f8842e4175909a04de062fa6b419a5d7da26822ea21a216280393ac1");
        let value = json!({
            "address": pool,
            "blockNumber": "0x9c008a",
            "transactionHash": hash,
            "transactionIndex": "0x2",
            "logIndex": "0x7",
            "topics": [V3_SWAP_TOPIC],
            "data": "0x"
        });
        let parsed = parse_observed_pool_swap_log(&value).unwrap();
        assert_eq!(parsed.l2_block_number, 10_223_754);
        assert_eq!(parsed.transaction_hash, hash);
        assert_eq!(parsed.log.address, pool);
        assert_eq!(parsed.log.log_index, 7);
    }

    #[test]
    fn retries_non_json_rate_limits_and_server_errors_before_decoding() {
        assert!(matches!(
            classify_rpc_response(StatusCode::TOO_MANY_REQUESTS, b"slow down"),
            RpcResponse::Retryable(reason) if reason.contains("429")
        ));
        assert!(matches!(
            classify_rpc_response(StatusCode::BAD_GATEWAY, b"<html>upstream failed</html>"),
            RpcResponse::Retryable(reason) if reason.contains("502")
        ));
    }

    #[test]
    fn retries_malformed_success_and_json_rpc_rate_limit() {
        assert!(matches!(
            classify_rpc_response(StatusCode::OK, b"truncated"),
            RpcResponse::Retryable(reason) if reason.contains("invalid JSON")
        ));
        assert!(matches!(
            classify_rpc_response(
                StatusCode::OK,
                br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32005,"message":"rate limited"}}"#,
            ),
            RpcResponse::Retryable(reason) if reason.contains("rate limit")
        ));
        assert!(response_is_rate_limited(
            StatusCode::OK,
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32005,"message":"rate limited"}}"#,
        ));
        assert!(response_is_rate_limited(
            StatusCode::TOO_MANY_REQUESTS,
            b"slow down",
        ));
    }

    #[test]
    fn returns_success_result_and_fatal_application_error() {
        assert_eq!(
            classify_rpc_response(
                StatusCode::OK,
                br#"{"jsonrpc":"2.0","id":1,"result":"0x2a"}"#,
            ),
            RpcResponse::Result(json!("0x2a"))
        );
        assert!(matches!(
            classify_rpc_response(
                StatusCode::OK,
                br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"bad params"}}"#,
            ),
            RpcResponse::Fatal(reason) if reason.contains("bad params")
        ));
    }

    #[test]
    fn strictly_decodes_token_snapshot_words() {
        let address = address!("955b339944cbd4834156366d766c260c80956b44");
        let mut address_word = [0_u8; 32];
        address_word[12..].copy_from_slice(address.as_slice());
        assert_eq!(parse_address_word(&address_word).unwrap(), address);

        address_word[0] = 1;
        assert!(parse_address_word(&address_word).is_err());
        assert!(parse_address_word(&address_word[..31]).is_err());

        let mut pool_fee_word = [0_u8; 32];
        pool_fee_word[28..].copy_from_slice(&10_000_u32.to_be_bytes());
        assert_eq!(parse_u32_word(&pool_fee_word).unwrap(), 10_000);
        pool_fee_word[0] = 1;
        assert!(parse_u32_word(&pool_fee_word).is_err());
    }
}
