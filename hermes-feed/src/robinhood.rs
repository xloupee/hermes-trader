use alloy_primitives::{Address, B256};

pub const CHAIN_ID: u64 = 4_663;
pub const TESTNET_CHAIN_ID: u64 = 46_630;

pub const PUBLIC_RPC_URL: &str = "https://rpc.mainnet.chain.robinhood.com";
pub const DIRECT_SEQUENCER_URL: &str = "https://sequencer.mainnet.chain.robinhood.com";
pub const DIRECT_FEED_URL: &str = "wss://feed.mainnet.chain.robinhood.com";
pub const TESTNET_RPC_URL: &str = "https://rpc.testnet.chain.robinhood.com";
pub const TESTNET_SEQUENCER_URL: &str = "https://sequencer.testnet.chain.robinhood.com";
pub const TESTNET_FEED_URL: &str = "wss://feed.testnet.chain.robinhood.com";

pub const NOXA_LAUNCH_FACTORY: Address =
    alloy_primitives::address!("d9ec2db5f3d1b236843925949fe5bd8a3836fccb");
pub const NOXA_LAUNCH_LOCKER: Address =
    alloy_primitives::address!("7f03effbd7ceb22a3f80dd468f67ef27826acd85");
/// Current N0xa deployment. This is deliberately separate from the retired
/// NOXA deployment above; callers must opt in to this factory explicitly.
pub const ACTIVE_NOXA_LAUNCH_FACTORY: Address =
    alloy_primitives::address!("52453b4289a6c3a70bb8b4682bcd3d8731267e28");
pub const ACTIVE_NOXA_LAUNCH_LOCKER: Address =
    alloy_primitives::address!("173d8370b4f67535d406f2f46168ec48aa03d26e");
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
/// Upgradeable frontend aggregator used by Robinhood Chain wallets. Hermes
/// observes this address but always submits follower swaps to SwapRouter02.
pub const ROBINHOOD_SWAP_AGGREGATOR: Address =
    alloy_primitives::address!("65050a9b7e5075a2ba5ced7b1b64ee66262c40dc");

pub const BOW_LAUNCH_FACTORY: Address =
    alloy_primitives::address!("c70e510e14710ea535cab7b2414860af63feab79");
pub const BOW_LAUNCH_LOCKER: Address =
    alloy_primitives::address!("904dccb96d877e6db365282251fa3dd156476660");
pub const BOW_LAUNCH_FACTORY_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("8d56cbcdf72dbf04ed8170d55878cc894997ccc54c2ab0aec782274eb7fe7a14");
pub const LAUNCHHOOD_V3_FACTORY: Address =
    alloy_primitives::address!("62b33a039d289cbda50ebeb72fe4261449e61bcf");
pub const LAUNCHHOOD_V3_LOCKER: Address =
    alloy_primitives::address!("99b79154ff4fc0e313549b809254b02722631ee0");
pub const LAUNCHHOOD_V3_TOKEN_IMPLEMENTATION: Address =
    alloy_primitives::address!("5fdf73abc7a232d91b03638c2f9a52c16ab0e3be");

pub const NOXA_DEX_ID_UNISWAP: u64 = 0;
pub const NOXA_LAUNCH_CONFIG_ID_WETH: u64 = 0;
pub const NOXA_POOL_FEE: u32 = 10_000;
pub const NOXA_TICK_SPACING: i32 = 200;
pub const NOXA_RESTRICTION_L1_BLOCKS: u64 = 366;
pub const NOXA_FACTORY_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("adcfca67f5d7df9f26974a07be2b5d83894765e6e5e9b9f0a232223f25c795e6");
pub const UNISWAP_V3_FACTORY_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("ec72b1abd1f2faee020cfea9c646bd8994f9fb389054f6e574f103a895091739");
pub const WETH_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("5706be52f64875fee65a2cec0d80e47a23d8793cbe85d214b48445e2d05f5353");
pub const UNISWAP_V3_POSITION_MANAGER_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("0a493d1af3d0f25fed8efa205244ebee14114267a08647fc38c515c7cd6ead4f");
pub const NOXA_TOKEN_CREATION_CODE_KECCAK256: B256 =
    alloy_primitives::b256!("983cd2b9ed743ddb79121ba35310b8eb55440997f88b0de4b0705ea5463ec7e1");
pub const ACTIVE_NOXA_FACTORY_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("f4c5e57d72e716c1da2dbe6c598a26b43528fb27fe234534c15527b47fae4a67");
pub const ACTIVE_NOXA_TOKEN_CREATION_CODE_KECCAK256: B256 =
    alloy_primitives::b256!("138947820ae2a1381e56ac03e883ad3e42b2701fbf5577976015c0944d335ccc");
pub const UNISWAP_V3_POOL_INIT_CODE_KECCAK256: B256 =
    alloy_primitives::b256!("e34f199b19b2b4f47f68442619d555527d244f78a3297ea89325f843f87b8b54");
pub const UNISWAP_V3_POOL_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("42e2b85666f65feb5b70bf6b61cd156d30a9997d00c0bcca3c16d54c92ea43d6");
pub const UNISWAP_V3_SWAP_ROUTER_02_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("6f36c378e272c6324c48f045182bcb54bd8ad654cf9ebd42e8893d52c4cb25dc");
