use alloy_primitives::{Address, B256, Bytes, I256, U256, ruint::UintTryFrom};
use alloy_sol_types::{SolCall, SolEvent, sol};
use serde::{Deserialize, Serialize};

sol! {
    struct Socials {
        string telegram;
        string twitter;
        string discord;
        string website;
        string farcaster;
    }

    struct LaunchParams {
        string name;
        string symbol;
        string logo;
        string description;
        Socials socials;
        address devWallet;
    }

    function launchToken(
        LaunchParams params,
        uint256 launchConfigId,
        uint256 dexId,
        bytes32 salt
    ) external payable returns (address token, uint256 positionId);

    event TokenLaunched(
        address indexed token,
        address indexed deployer,
        address indexed dexFactory,
        address pairToken,
        address pool,
        uint256 dexId,
        uint256 launchConfigId,
        uint256 positionId,
        uint256 restrictionsEndBlock,
        uint256 initialBuyAmount
    );

    event PoolCreated(
        address indexed token0,
        address indexed token1,
        uint24 indexed fee,
        int24 tickSpacing,
        address pool
    );

    event Initialize(uint160 sqrtPriceX96, int24 tick);

    event Mint(
        address sender,
        address indexed owner,
        int24 indexed tickLower,
        int24 indexed tickUpper,
        uint128 amount,
        uint256 amount0,
        uint256 amount1
    );

    event Burn(
        address indexed owner,
        int24 indexed tickLower,
        int24 indexed tickUpper,
        uint128 amount,
        uint256 amount0,
        uint256 amount1
    );

    event Swap(
        address indexed sender,
        address indexed recipient,
        int256 amount0,
        int256 amount1,
        uint160 sqrtPriceX96,
        uint128 liquidity,
        int24 tick
    );

    struct ExactInputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 amountIn;
        uint256 amountOutMinimum;
        uint160 sqrtPriceLimitX96;
    }

    function exactInputSingle(ExactInputSingleParams params)
        external
        payable
        returns (uint256 amountOut);

    struct ExactOutputSingleParams {
        address tokenIn;
        address tokenOut;
        uint24 fee;
        address recipient;
        uint256 amountOut;
        uint256 amountInMaximum;
        uint160 sqrtPriceLimitX96;
    }

    function exactOutputSingle(ExactOutputSingleParams params)
        external
        payable
        returns (uint256 amountIn);
}

pub const LAUNCH_TOKEN_SELECTOR: [u8; 4] = [0x68, 0x63, 0x99, 0xcb];
pub const EXACT_INPUT_SINGLE_SELECTOR: [u8; 4] = [0x04, 0xe4, 0x5a, 0xaf];
pub const EXACT_OUTPUT_SINGLE_SELECTOR: [u8; 4] = [0x50, 0x23, 0xb4, 0xdf];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoxaSocials {
    pub telegram: String,
    pub twitter: String,
    pub discord: String,
    pub website: String,
    pub farcaster: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoxaLaunchIntent {
    pub name: String,
    pub symbol: String,
    pub logo: String,
    pub description: String,
    pub socials: NoxaSocials,
    pub dev_wallet: Address,
    pub launch_config_id: U256,
    pub dex_id: U256,
    pub salt: B256,
    pub transaction_value: U256,
}

/// Fixed launchToken calldata fields used by the feed hot path.
///
/// Dynamic metadata is deliberately left undecoded until the receipt proves
/// that the transaction succeeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoxaLaunchHeader {
    pub launch_config_id: U256,
    pub dex_id: U256,
    pub salt: B256,
    pub transaction_value: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct NoxaLaunchEvent {
    pub token: Address,
    pub deployer: Address,
    pub dex_factory: Address,
    pub pair_token: Address,
    pub pool: Address,
    pub dex_id: U256,
    pub launch_config_id: U256,
    pub position_id: U256,
    pub restrictions_end_l1_block: U256,
    pub initial_buy_amount: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3ExactInputIntent {
    pub token_in: Address,
    pub token_out: Address,
    pub fee: u32,
    pub recipient: Address,
    pub amount_in: U256,
    pub amount_out_minimum: U256,
    pub sqrt_price_limit_x96: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3ExactOutputIntent {
    pub token_in: Address,
    pub token_out: Address,
    pub fee: u32,
    pub recipient: Address,
    pub amount_out: U256,
    pub amount_in_maximum: U256,
    pub sqrt_price_limit_x96: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ReceiptLog {
    pub address: Address,
    pub log_index: u64,
    pub topics: Vec<B256>,
    pub data: Bytes,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V3PoolCreatedEvent {
    pub token0: Address,
    pub token1: Address,
    pub fee: u32,
    pub tick_spacing: i32,
    pub pool: Address,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum V3PoolEvent {
    Initialize {
        sqrt_price_x96: U256,
        tick: i32,
    },
    Mint {
        tick_lower: i32,
        tick_upper: i32,
        amount: u128,
    },
    Burn {
        tick_lower: i32,
        tick_upper: i32,
        amount: u128,
    },
    Swap {
        sender: Address,
        recipient: Address,
        amount0: I256,
        amount1: I256,
        sqrt_price_x96: U256,
        liquidity: u128,
        tick: i32,
    },
}

pub fn decode_launch_call(input: &[u8], transaction_value: U256) -> Option<NoxaLaunchIntent> {
    if input.get(..4)? != LAUNCH_TOKEN_SELECTOR {
        return None;
    }
    let call = launchTokenCall::abi_decode(input).ok()?;
    Some(NoxaLaunchIntent {
        name: call.params.name,
        symbol: call.params.symbol,
        logo: call.params.logo,
        description: call.params.description,
        socials: NoxaSocials {
            telegram: call.params.socials.telegram,
            twitter: call.params.socials.twitter,
            discord: call.params.socials.discord,
            website: call.params.socials.website,
            farcaster: call.params.socials.farcaster,
        },
        dev_wallet: call.params.devWallet,
        launch_config_id: call.launchConfigId,
        dex_id: call.dexId,
        salt: call.salt,
        transaction_value,
    })
}

pub fn decode_launch_header(input: &[u8], transaction_value: U256) -> Option<NoxaLaunchHeader> {
    let head = input.get(..132)?;
    if head[..4] != LAUNCH_TOKEN_SELECTOR {
        return None;
    }
    // launchToken has four top-level arguments. The first is a dynamic tuple,
    // so canonical ABI calldata points it immediately past the four-word head.
    if U256::from_be_slice(&head[4..36]) != U256::from(128) {
        return None;
    }
    Some(NoxaLaunchHeader {
        launch_config_id: U256::from_be_slice(&head[36..68]),
        dex_id: U256::from_be_slice(&head[68..100]),
        salt: B256::from_slice(&head[100..132]),
        transaction_value,
    })
}

pub fn decode_v3_exact_input_single(input: &[u8]) -> Option<V3ExactInputIntent> {
    let call = exactInputSingleCall::abi_decode(input).ok()?;
    if call.abi_encode().as_slice() != input {
        return None;
    }
    Some(V3ExactInputIntent {
        token_in: call.params.tokenIn,
        token_out: call.params.tokenOut,
        fee: u32::try_from(call.params.fee).ok()?,
        recipient: call.params.recipient,
        amount_in: call.params.amountIn,
        amount_out_minimum: call.params.amountOutMinimum,
        sqrt_price_limit_x96: U256::from(call.params.sqrtPriceLimitX96),
    })
}

pub fn decode_v3_exact_output_single(input: &[u8]) -> Option<V3ExactOutputIntent> {
    let call = exactOutputSingleCall::abi_decode(input).ok()?;
    if call.abi_encode().as_slice() != input {
        return None;
    }
    Some(V3ExactOutputIntent {
        token_in: call.params.tokenIn,
        token_out: call.params.tokenOut,
        fee: u32::try_from(call.params.fee).ok()?,
        recipient: call.params.recipient,
        amount_out: call.params.amountOut,
        amount_in_maximum: call.params.amountInMaximum,
        sqrt_price_limit_x96: U256::from(call.params.sqrtPriceLimitX96),
    })
}

pub fn decode_token_launched(log: &ReceiptLog) -> Option<NoxaLaunchEvent> {
    let decoded =
        TokenLaunched::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()?;
    Some(NoxaLaunchEvent {
        token: decoded.token,
        deployer: decoded.deployer,
        dex_factory: decoded.dexFactory,
        pair_token: decoded.pairToken,
        pool: decoded.pool,
        dex_id: decoded.dexId,
        launch_config_id: decoded.launchConfigId,
        position_id: decoded.positionId,
        restrictions_end_l1_block: decoded.restrictionsEndBlock,
        initial_buy_amount: decoded.initialBuyAmount,
    })
}

pub fn decode_pool_created(log: &ReceiptLog) -> Option<V3PoolCreatedEvent> {
    let decoded =
        PoolCreated::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()?;
    Some(V3PoolCreatedEvent {
        token0: decoded.token0,
        token1: decoded.token1,
        fee: u32::try_from(decoded.fee).ok()?,
        tick_spacing: i32::try_from(decoded.tickSpacing).ok()?,
        pool: decoded.pool,
    })
}

pub fn decode_v3_pool_event(log: &ReceiptLog) -> Option<V3PoolEvent> {
    let topic = *log.topics.first()?;
    if topic == Initialize::SIGNATURE_HASH {
        let event =
            Initialize::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()?;
        return Some(V3PoolEvent::Initialize {
            sqrt_price_x96: U256::from(event.sqrtPriceX96),
            tick: i32::try_from(event.tick).ok()?,
        });
    }
    if topic == Mint::SIGNATURE_HASH {
        let event = Mint::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()?;
        return Some(V3PoolEvent::Mint {
            tick_lower: i32::try_from(event.tickLower).ok()?,
            tick_upper: i32::try_from(event.tickUpper).ok()?,
            amount: event.amount,
        });
    }
    if topic == Burn::SIGNATURE_HASH {
        let event = Burn::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()?;
        return Some(V3PoolEvent::Burn {
            tick_lower: i32::try_from(event.tickLower).ok()?,
            tick_upper: i32::try_from(event.tickUpper).ok()?,
            amount: event.amount,
        });
    }
    if topic == Swap::SIGNATURE_HASH {
        let event = Swap::decode_raw_log_validate(log.topics.iter().copied(), &log.data).ok()?;
        return Some(V3PoolEvent::Swap {
            sender: event.sender,
            recipient: event.recipient,
            amount0: event.amount0,
            amount1: event.amount1,
            sqrt_price_x96: U256::from(event.sqrtPriceX96),
            liquidity: event.liquidity,
            tick: i32::try_from(event.tick).ok()?,
        });
    }
    None
}

pub fn encode_v3_exact_input_single(intent: &V3ExactInputIntent) -> Option<Vec<u8>> {
    if intent.token_in == Address::ZERO
        || intent.token_out == Address::ZERO
        || intent.token_in == intent.token_out
        || intent.recipient == Address::ZERO
        || intent.fee > 0x00ff_ffff
        || intent.amount_in == U256::ZERO
        || intent.amount_out_minimum == U256::ZERO
    {
        return None;
    }
    let sqrt_price_limit_x96 =
        alloy_primitives::aliases::U160::uint_try_from(intent.sqrt_price_limit_x96).ok()?;
    Some(
        exactInputSingleCall {
            params: ExactInputSingleParams {
                tokenIn: intent.token_in,
                tokenOut: intent.token_out,
                fee: alloy_primitives::aliases::U24::from(intent.fee),
                recipient: intent.recipient,
                amountIn: intent.amount_in,
                amountOutMinimum: intent.amount_out_minimum,
                sqrtPriceLimitX96: sqrt_price_limit_x96,
            },
        }
        .abi_encode(),
    )
}

pub fn encode_v3_exact_output_single(intent: &V3ExactOutputIntent) -> Option<Vec<u8>> {
    if intent.token_in == Address::ZERO
        || intent.token_out == Address::ZERO
        || intent.token_in == intent.token_out
        || intent.recipient == Address::ZERO
        || intent.fee > 0x00ff_ffff
        || intent.amount_out == U256::ZERO
        || intent.amount_in_maximum == U256::ZERO
    {
        return None;
    }
    let sqrt_price_limit_x96 =
        alloy_primitives::aliases::U160::uint_try_from(intent.sqrt_price_limit_x96).ok()?;
    Some(
        exactOutputSingleCall {
            params: ExactOutputSingleParams {
                tokenIn: intent.token_in,
                tokenOut: intent.token_out,
                fee: alloy_primitives::aliases::U24::from(intent.fee),
                recipient: intent.recipient,
                amountOut: intent.amount_out,
                amountInMaximum: intent.amount_in_maximum,
                sqrtPriceLimitX96: sqrt_price_limit_x96,
            },
        }
        .abi_encode(),
    )
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{FixedBytes, address, b256};

    use super::*;

    #[test]
    fn selectors_match_live_robinhood_contracts() {
        assert_eq!(launchTokenCall::SELECTOR, LAUNCH_TOKEN_SELECTOR);
        assert_eq!(exactInputSingleCall::SELECTOR, EXACT_INPUT_SINGLE_SELECTOR);
        assert_eq!(
            exactOutputSingleCall::SELECTOR,
            EXACT_OUTPUT_SINGLE_SELECTOR
        );
        assert_eq!(
            TokenLaunched::SIGNATURE_HASH,
            b256!("db51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a")
        );
    }

    #[test]
    fn decodes_live_launch_event() {
        let log = ReceiptLog {
            address: address!("d9ec2db5f3d1b236843925949fe5bd8a3836fccb"),
            log_index: 0x20,
            topics: vec![
                TokenLaunched::SIGNATURE_HASH,
                B256::left_padding_from(
                    address!("955b339944cbd4834156366d766c260c80956b44").as_slice(),
                ),
                B256::left_padding_from(
                    address!("4ba04830e5f615dc0e7d80a7dc4352c241ccbdc2").as_slice(),
                ),
                B256::left_padding_from(
                    address!("1f7d7550b1b028f7571e69a784071f0205fd2efa").as_slice(),
                ),
            ],
            data: Bytes::from(
                hex::decode(concat!(
                    "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad73",
                    "000000000000000000000000efd703d89b7febc0ae43fdd72edd257819366272",
                    "0000000000000000000000000000000000000000000000000000000000000000",
                    "0000000000000000000000000000000000000000000000000000000000000000",
                    "000000000000000000000000000000000000000000000000000000000001339e",
                    "0000000000000000000000000000000000000000000000000000000001853d61",
                    "00000000000000000000000000000000000000000000000000b1a2bc2ec50000"
                ))
                .unwrap(),
            ),
        };

        let event = decode_token_launched(&log).unwrap();
        assert_eq!(
            event.token,
            address!("955b339944cbd4834156366d766c260c80956b44")
        );
        assert_eq!(
            event.pool,
            address!("efd703d89b7febc0ae43fdd72edd257819366272")
        );
        assert_eq!(event.position_id, U256::from(78_750));
        assert_eq!(event.restrictions_end_l1_block, U256::from(25_509_217));
        assert_eq!(
            event.initial_buy_amount,
            U256::from(50_000_000_000_000_000_u64)
        );
    }

    #[test]
    fn exact_input_single_round_trips() {
        let intent = V3ExactInputIntent {
            token_in: address!("0bd7d308f8e1639fab988df18a8011f41eacad73"),
            token_out: address!("955b339944cbd4834156366d766c260c80956b44"),
            fee: 10_000,
            recipient: Address::with_last_byte(7),
            amount_in: U256::from(12_000_000_000_000_000_u64),
            amount_out_minimum: U256::from(1),
            sqrt_price_limit_x96: U256::ZERO,
        };
        let encoded = encode_v3_exact_input_single(&intent).unwrap();
        assert_eq!(&encoded[..4], &EXACT_INPUT_SINGLE_SELECTOR);
        let decoded = exactInputSingleCall::abi_decode(&encoded).unwrap();
        assert_eq!(decoded.params.tokenIn, intent.token_in);
        assert_eq!(decoded.params.amountIn, intent.amount_in);
        assert_eq!(decode_v3_exact_input_single(&encoded), Some(intent));

        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_v3_exact_input_single(&trailing).is_none());
    }

    #[test]
    fn exact_output_single_round_trips() {
        let intent = V3ExactOutputIntent {
            token_in: address!("0bd7d308f8e1639fab988df18a8011f41eacad73"),
            token_out: address!("955b339944cbd4834156366d766c260c80956b44"),
            fee: 10_000,
            recipient: Address::with_last_byte(7),
            amount_out: U256::from(20_000_000_000_000_000_000_u128),
            amount_in_maximum: U256::from(12_000_000_000_000_000_u64),
            sqrt_price_limit_x96: U256::ZERO,
        };
        let encoded = encode_v3_exact_output_single(&intent).unwrap();
        assert_eq!(&encoded[..4], &EXACT_OUTPUT_SINGLE_SELECTOR);
        let decoded = exactOutputSingleCall::abi_decode(&encoded).unwrap();
        assert_eq!(decoded.params.amountOut, intent.amount_out);
        assert_eq!(decoded.params.amountInMaximum, intent.amount_in_maximum);
        assert_eq!(decode_v3_exact_output_single(&encoded), Some(intent));
    }

    #[test]
    fn launch_call_round_trips() {
        let call = launchTokenCall {
            params: LaunchParams {
                name: "Hermes".into(),
                symbol: "HERMES".into(),
                logo: "ipfs://logo".into(),
                description: "test".into(),
                socials: Socials {
                    telegram: String::new(),
                    twitter: "https://x.com/hermes".into(),
                    discord: String::new(),
                    website: String::new(),
                    farcaster: String::new(),
                },
                devWallet: Address::with_last_byte(9),
            },
            launchConfigId: U256::ZERO,
            dexId: U256::ZERO,
            salt: FixedBytes::with_last_byte(11),
        };
        let encoded = call.abi_encode();
        let intent = decode_launch_call(&encoded, U256::from(123)).unwrap();
        let header = decode_launch_header(&encoded, U256::from(123)).unwrap();
        assert_eq!(intent.name, "Hermes");
        assert_eq!(intent.symbol, "HERMES");
        assert_eq!(intent.salt, FixedBytes::with_last_byte(11));
        assert_eq!(intent.transaction_value, U256::from(123));
        assert_eq!(header.launch_config_id, intent.launch_config_id);
        assert_eq!(header.dex_id, intent.dex_id);
        assert_eq!(header.salt, intent.salt);
        assert_eq!(header.transaction_value, intent.transaction_value);
    }

    #[test]
    fn launch_header_rejects_noncanonical_or_truncated_heads() {
        let mut head = vec![0_u8; 132];
        head[..4].copy_from_slice(&LAUNCH_TOKEN_SELECTOR);
        head[35] = 128;
        assert!(decode_launch_header(&head, U256::ZERO).is_some());

        head[35] = 127;
        assert!(decode_launch_header(&head, U256::ZERO).is_none());
        assert!(decode_launch_header(&head[..131], U256::ZERO).is_none());

        head[0] ^= 1;
        assert!(decode_launch_header(&head, U256::ZERO).is_none());
    }
}
