use alloy_primitives::{Address, U256};
use serde::Serialize;
use thiserror::Error;

use crate::noxa_abi::{V3ExactInputIntent, decode_aggregator_swap};
use crate::noxa_predict::predict_v3_pool_address;
use crate::robinhood::{
    NOXA_POOL_FEE, NOXA_TICK_SPACING, UNISWAP_V3_FACTORY, UNISWAP_V3_POOL_INIT_CODE_KECCAK256, WETH,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedAggregatorSwap {
    pub intent: V3ExactInputIntent,
    pub pool: Address,
    pub leader_used_native_eth: bool,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregatorCopyRejectReason {
    #[error("aggregator calldata is not canonical")]
    Malformed,
    #[error("aggregator route must contain exactly one swap leg")]
    MultiLeg,
    #[error("aggregator fee token or extension fields are unsupported")]
    Extensions,
    #[error("aggregator route is not a NOXA WETH/token exact-input swap")]
    UnsupportedPair,
    #[error("aggregator route does not name the canonical NOXA V3 pool")]
    WrongPool,
    #[error("aggregator input value does not match its declared amount")]
    WrongValue,
    #[error("aggregator amounts must be non-zero")]
    ZeroAmount,
}

pub fn normalize_aggregator_copy_swap(
    input: &[u8],
    transaction_value: U256,
    leader: Address,
) -> Result<NormalizedAggregatorSwap, AggregatorCopyRejectReason> {
    let call = decode_aggregator_swap(input).ok_or(AggregatorCopyRejectReason::Malformed)?;
    let [leg] = call.descriptors.as_slice() else {
        return Err(AggregatorCopyRejectReason::MultiLeg);
    };
    if call.fee_token != Address::ZERO
        || leg.router != Address::ZERO
        || leg.callback != Address::ZERO
        || leg.metadata != alloy_primitives::B256::ZERO
    {
        return Err(AggregatorCopyRejectReason::Extensions);
    }
    let leader_used_native_eth = transaction_value != U256::ZERO;
    let native_flag = decode_native_flag(&leg.data, leader_used_native_eth)
        .ok_or(AggregatorCopyRejectReason::Extensions)?;
    if call.amount_in == U256::ZERO || call.minimum_return == U256::ZERO {
        return Err(AggregatorCopyRejectReason::ZeroAmount);
    }
    if leg.fee != NOXA_POOL_FEE
        || !matches!(leg.tick_spacing, 0 | NOXA_TICK_SPACING)
        || !is_weth_pair(leg.token_in, leg.token_out)
    {
        return Err(AggregatorCopyRejectReason::UnsupportedPair);
    }
    let (token0, token1) = if leg.token_in < leg.token_out {
        (leg.token_in, leg.token_out)
    } else {
        (leg.token_out, leg.token_in)
    };
    let expected_pool = predict_v3_pool_address(
        UNISWAP_V3_FACTORY,
        token0,
        token1,
        NOXA_POOL_FEE,
        UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
    );
    if leg.pool != expected_pool {
        return Err(AggregatorCopyRejectReason::WrongPool);
    }

    let is_entry = leg.token_in == WETH;
    if (is_entry && leader_used_native_eth && transaction_value != call.amount_in)
        || (!is_entry && leader_used_native_eth)
        || native_flag != leader_used_native_eth
    {
        return Err(AggregatorCopyRejectReason::WrongValue);
    }

    Ok(NormalizedAggregatorSwap {
        intent: V3ExactInputIntent {
            token_in: leg.token_in,
            token_out: leg.token_out,
            fee: leg.fee,
            recipient: leader,
            amount_in: call.amount_in,
            amount_out_minimum: call.minimum_return,
            sqrt_price_limit_x96: U256::ZERO,
        },
        pool: leg.pool,
        leader_used_native_eth,
    })
}

fn decode_native_flag(data: &[u8], inferred_from_value: bool) -> Option<bool> {
    // Current Robinhood aggregator calldata omits the legacy redundant flag.
    // Native input remains unambiguous because transaction value must exactly
    // equal amountIn for an entry and must be zero for every token exit.
    if data.is_empty() {
        return Some(inferred_from_value);
    }
    let word: &[u8; 32] = data.try_into().ok()?;
    if word[..31] != [0; 31] {
        return None;
    }
    match word[31] {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn is_weth_pair(token_in: Address, token_out: Address) -> bool {
    token_in != Address::ZERO
        && token_out != Address::ZERO
        && token_in != token_out
        && ((token_in == WETH) ^ (token_out == WETH))
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{
        B256, Bytes,
        aliases::{I24, U24},
    };
    use alloy_sol_types::{SolCall, sol};

    use super::*;

    sol! {
        struct TestDescriptor {
            uint8 dexId;
            address tokenIn;
            address tokenOut;
            address pool;
            uint24 fee;
            int24 tickSpacing;
            address router;
            bytes data;
            address callback;
            bytes32 metadata;
        }

        function swap(
            TestDescriptor[] descs,
            address feeToken,
            uint256 amountIn,
            uint256 minReturn,
            uint256 userFeeRate
        ) external payable;
    }

    fn token() -> Address {
        Address::with_last_byte(0x42)
    }

    fn pool() -> Address {
        let (token0, token1) = if WETH < token() {
            (WETH, token())
        } else {
            (token(), WETH)
        };
        predict_v3_pool_address(
            UNISWAP_V3_FACTORY,
            token0,
            token1,
            NOXA_POOL_FEE,
            UNISWAP_V3_POOL_INIT_CODE_KECCAK256,
        )
    }

    fn calldata_with_route_data(
        token_in: Address,
        token_out: Address,
        route_data: Vec<u8>,
    ) -> Vec<u8> {
        swapCall {
            descs: vec![TestDescriptor {
                dexId: 5,
                tokenIn: token_in,
                tokenOut: token_out,
                pool: pool(),
                fee: U24::from(NOXA_POOL_FEE),
                tickSpacing: I24::try_from(0).unwrap(),
                router: Address::ZERO,
                data: Bytes::from(route_data),
                callback: Address::ZERO,
                metadata: B256::ZERO,
            }],
            feeToken: Address::ZERO,
            amountIn: U256::from(100),
            minReturn: U256::from(250),
            userFeeRate: U256::ZERO,
        }
        .abi_encode()
    }

    fn calldata(token_in: Address, token_out: Address, native: bool) -> Vec<u8> {
        let mut route_data = vec![0_u8; 32];
        route_data[31] = u8::from(native);
        calldata_with_route_data(token_in, token_out, route_data)
    }

    #[test]
    fn normalizes_native_entry_to_a_direct_v3_intent() {
        let leader = Address::with_last_byte(7);
        let normalized =
            normalize_aggregator_copy_swap(&calldata(WETH, token(), true), U256::from(100), leader)
                .unwrap();
        assert_eq!(normalized.pool, pool());
        assert!(normalized.leader_used_native_eth);
        assert_eq!(normalized.intent.token_in, WETH);
        assert_eq!(normalized.intent.token_out, token());
        assert_eq!(normalized.intent.recipient, leader);
        assert_eq!(normalized.intent.amount_in, U256::from(100));
        assert_eq!(normalized.intent.amount_out_minimum, U256::from(250));
    }

    #[test]
    fn normalizes_current_empty_data_value_convention_and_still_checks_value() {
        let leader = Address::with_last_byte(7);
        let entry = calldata_with_route_data(WETH, token(), Vec::new());
        let native = normalize_aggregator_copy_swap(&entry, U256::from(100), leader).unwrap();
        assert!(native.leader_used_native_eth);
        assert_eq!(native.intent.amount_in, U256::from(100));
        assert_eq!(
            normalize_aggregator_copy_swap(&entry, U256::from(99), leader),
            Err(AggregatorCopyRejectReason::WrongValue)
        );

        let exit = calldata_with_route_data(token(), WETH, Vec::new());
        assert!(normalize_aggregator_copy_swap(&exit, U256::ZERO, leader).is_ok());
        assert_eq!(
            normalize_aggregator_copy_swap(&exit, U256::from(100), leader),
            Err(AggregatorCopyRejectReason::WrongValue)
        );
    }

    #[test]
    fn normalizes_token_exit_and_rejects_native_exit() {
        let leader = Address::with_last_byte(7);
        let encoded = calldata(token(), WETH, false);
        assert!(normalize_aggregator_copy_swap(&encoded, U256::ZERO, leader).is_ok());
        assert_eq!(
            normalize_aggregator_copy_swap(&encoded, U256::from(100), leader),
            Err(AggregatorCopyRejectReason::WrongValue)
        );
    }

    #[test]
    fn rejects_a_noncanonical_pool() {
        let mut encoded = calldata(WETH, token(), true);
        // The descriptor pool is word 10 for a one-element dynamic array.
        encoded[4 + 10 * 32 + 31] ^= 1;
        assert!(
            normalize_aggregator_copy_swap(&encoded, U256::from(100), Address::with_last_byte(7))
                .is_err()
        );
    }

    #[test]
    fn decodes_a_live_watched_wallet_native_buy() {
        // Robinhood Chain tx 0x43b65853a0c2fcdcdbb5f2d900ecb102b9b017907a8aff183bb95f00d0e3ac55b.
        let input = hex::decode(concat!(
            "4d819a2a00000000000000000000000000000000000000000000000000000000000000a0",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "000000000000000000000000000000000000000000000000016345785d8a0000",
            "00000000000000000000000000000000000000000002927e5b186c39569a8a56",
            "000000000000000000000000000000000000000000000000000000006a555502",
            "0000000000000000000000000000000000000000000000000000000000000001",
            "0000000000000000000000000000000000000000000000000000000000000020",
            "0000000000000000000000000000000000000000000000000000000000000005",
            "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad73",
            "000000000000000000000000305611a943b83bcda72a12106b949a27a95f5b16",
            "00000000000000000000000021f93d1c92d0b7ae705fa8d3ddbfa4a6c4271ca4",
            "0000000000000000000000000000000000000000000000000000000000002710",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000140",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "0000000000000000000000000000000000000000000000000000000000000020",
            "0000000000000000000000000000000000000000000000000000000000000001",
        ))
        .unwrap();
        let leader =
            Address::from_slice(&hex::decode("9938e392367cd196034f07c325aa22ee0bf31a85").unwrap());
        let normalized =
            normalize_aggregator_copy_swap(&input, U256::from(100_000_000_000_000_000_u64), leader)
                .unwrap();
        assert!(normalized.leader_used_native_eth);
        assert_eq!(normalized.intent.token_in, WETH);
        assert_eq!(normalized.intent.recipient, leader);
        assert_eq!(
            normalized.intent.amount_in,
            U256::from(100_000_000_000_000_000_u64)
        );
        assert_eq!(normalized.pool, expected_live_pool());
    }

    fn expected_live_pool() -> Address {
        Address::from_slice(&hex::decode("21f93d1c92d0b7ae705fa8d3ddbfa4a6c4271ca4").unwrap())
    }
}
