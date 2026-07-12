use alloy_primitives::{Address, B256};

pub const CHAIN_ID: u64 = 4_663;

pub const PUBLIC_RPC_URL: &str = "https://rpc.mainnet.chain.robinhood.com";
pub const DIRECT_SEQUENCER_URL: &str = "https://sequencer.mainnet.chain.robinhood.com";
pub const DIRECT_FEED_URL: &str = "wss://feed.mainnet.chain.robinhood.com";

pub const NOXA_LAUNCH_FACTORY: Address =
    alloy_primitives::address!("d9ec2db5f3d1b236843925949fe5bd8a3836fccb");
pub const NOXA_LAUNCH_LOCKER: Address =
    alloy_primitives::address!("7f03effbd7ceb22a3f80dd468f67ef27826acd85");
pub const WETH: Address = alloy_primitives::address!("0bd7d308f8e1639fab988df18a8011f41eacad73");
pub const MULTICALL3: Address =
    alloy_primitives::address!("ca11bde05977b3631167028862be2a173976ca11");

pub const UNISWAP_V3_FACTORY: Address =
    alloy_primitives::address!("1f7d7550b1b028f7571e69a784071f0205fd2efa");
pub const UNISWAP_V3_POSITION_MANAGER: Address =
    alloy_primitives::address!("73991a25c818bf1f1128deaab1492d45638de0d3");
pub const UNISWAP_V3_QUOTER_V2: Address =
    alloy_primitives::address!("33e885ed0ec9bf04ecfb19341582aadcb4c8a9e7");
pub const UNISWAP_V3_SWAP_ROUTER_02: Address =
    alloy_primitives::address!("caf681a66d020601342297493863e78c959e5cb2");
pub const UNISWAP_UNIVERSAL_ROUTER: Address =
    alloy_primitives::address!("8876789976decbfcbbbe364623c63652db8c0904");

pub const NOXA_DEX_ID_UNISWAP: u64 = 0;
pub const NOXA_LAUNCH_CONFIG_ID_WETH: u64 = 0;
pub const NOXA_POOL_FEE: u32 = 10_000;
pub const NOXA_TICK_SPACING: i32 = 200;
pub const NOXA_RESTRICTION_L1_BLOCKS: u64 = 366;
pub const NOXA_FACTORY_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("adcfca67f5d7df9f26974a07be2b5d83894765e6e5e9b9f0a232223f25c795e6");
