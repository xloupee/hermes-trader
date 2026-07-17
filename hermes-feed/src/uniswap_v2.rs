use alloy_primitives::{Address, U256};
use alloy_sol_types::{SolCall, sol};
use serde::{Deserialize, Serialize};

sol! {
    function swapExactTokensForTokens(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external returns (uint256[] amounts);

    function swapExactETHForTokens(
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external payable returns (uint256[] amounts);

    function swapExactTokensForETH(
        uint256 amountIn,
        uint256 amountOutMin,
        address[] path,
        address to,
        uint256 deadline
    ) external returns (uint256[] amounts);
}

pub const SWAP_EXACT_TOKENS_FOR_TOKENS_SELECTOR: [u8; 4] = [0x38, 0xed, 0x17, 0x39];
pub const SWAP_EXACT_ETH_FOR_TOKENS_SELECTOR: [u8; 4] = [0x7f, 0xf3, 0x6a, 0xb5];
pub const SWAP_EXACT_TOKENS_FOR_ETH_SELECTOR: [u8; 4] = [0x18, 0xcb, 0xaf, 0xe5];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum V2SwapKind {
    TokensForTokens,
    EthForTokens,
    TokensForEth,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct V2SwapIntent {
    pub kind: V2SwapKind,
    pub amount_in: U256,
    pub amount_out_min: U256,
    pub path: Vec<Address>,
    pub recipient: Address,
    pub deadline: U256,
}

pub fn decode_v2_exact_input(input: &[u8], transaction_value: U256) -> Option<V2SwapIntent> {
    let selector: [u8; 4] = input.get(..4)?.try_into().ok()?;
    match selector {
        SWAP_EXACT_TOKENS_FOR_TOKENS_SELECTOR => {
            let call = swapExactTokensForTokensCall::abi_decode(input).ok()?;
            valid_path(&call.path).then_some(V2SwapIntent {
                kind: V2SwapKind::TokensForTokens,
                amount_in: call.amountIn,
                amount_out_min: call.amountOutMin,
                path: call.path,
                recipient: call.to,
                deadline: call.deadline,
            })
        }
        SWAP_EXACT_ETH_FOR_TOKENS_SELECTOR => {
            let call = swapExactETHForTokensCall::abi_decode(input).ok()?;
            valid_path(&call.path).then_some(V2SwapIntent {
                kind: V2SwapKind::EthForTokens,
                amount_in: transaction_value,
                amount_out_min: call.amountOutMin,
                path: call.path,
                recipient: call.to,
                deadline: call.deadline,
            })
        }
        SWAP_EXACT_TOKENS_FOR_ETH_SELECTOR => {
            let call = swapExactTokensForETHCall::abi_decode(input).ok()?;
            valid_path(&call.path).then_some(V2SwapIntent {
                kind: V2SwapKind::TokensForEth,
                amount_in: call.amountIn,
                amount_out_min: call.amountOutMin,
                path: call.path,
                recipient: call.to,
                deadline: call.deadline,
            })
        }
        _ => None,
    }
}

fn valid_path(path: &[Address]) -> bool {
    path.len() >= 2 && path.iter().all(|address| *address != Address::ZERO)
}

#[cfg(test)]
mod tests {
    use alloy_sol_types::SolCall;

    use super::*;

    fn addresses() -> (Address, Address, Address) {
        (
            Address::with_last_byte(1),
            Address::with_last_byte(2),
            Address::with_last_byte(3),
        )
    }

    #[test]
    fn decodes_tokens_for_tokens() {
        let (token_in, token_out, recipient) = addresses();
        let encoded = swapExactTokensForTokensCall {
            amountIn: U256::from(10),
            amountOutMin: U256::from(9),
            path: vec![token_in, token_out],
            to: recipient,
            deadline: U256::from(123),
        }
        .abi_encode();

        let intent = decode_v2_exact_input(&encoded, U256::ZERO).unwrap();
        assert_eq!(intent.kind, V2SwapKind::TokensForTokens);
        assert_eq!(intent.amount_in, U256::from(10));
        assert_eq!(intent.amount_out_min, U256::from(9));
        assert_eq!(intent.path, vec![token_in, token_out]);
        assert_eq!(intent.recipient, recipient);
    }

    #[test]
    fn uses_transaction_value_for_eth_input() {
        let (weth, token_out, recipient) = addresses();
        let encoded = swapExactETHForTokensCall {
            amountOutMin: U256::from(9),
            path: vec![weth, token_out],
            to: recipient,
            deadline: U256::from(123),
        }
        .abi_encode();

        let intent = decode_v2_exact_input(&encoded, U256::from(10)).unwrap();
        assert_eq!(intent.kind, V2SwapKind::EthForTokens);
        assert_eq!(intent.amount_in, U256::from(10));
    }

    #[test]
    fn rejects_short_or_zero_address_paths() {
        let (_, token_out, recipient) = addresses();
        let short = swapExactTokensForETHCall {
            amountIn: U256::from(10),
            amountOutMin: U256::from(9),
            path: vec![token_out],
            to: recipient,
            deadline: U256::from(123),
        }
        .abi_encode();
        assert!(decode_v2_exact_input(&short, U256::ZERO).is_none());

        let zero = swapExactTokensForETHCall {
            amountIn: U256::from(10),
            amountOutMin: U256::from(9),
            path: vec![Address::ZERO, token_out],
            to: recipient,
            deadline: U256::from(123),
        }
        .abi_encode();
        assert!(decode_v2_exact_input(&zero, U256::ZERO).is_none());
    }
}
