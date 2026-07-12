# Uniswap v2 intent validation

Validation date: 2026-07-12

Configuration:

- Robinhood mainnet direct sequencer feed
- Canonical `V2Router02`: `0x89e5db8b5aa49aa85ac63f691524311aeb649eba`
- Selectors: `0x38ed1739`, `0x7ff36ab5`, `0x18cbafe5`
- No watched-wallet restriction; observation only
- Duration: 30 seconds, including feed catch-up

Result:

- Candidate transactions: 46
- Successfully typed v2 intents: 46
- Ambiguous/null intent decodes: 0
- Observed kind: 46 `eth_for_tokens`
- Intents with `amountOutMin == 0`: 3

The zero-minimum observations establish an explicit future paper/live policy:
Hermes must reject a target or generated copy with zero slippage protection.

## RPC cross-check

Sample transaction:
`0xb88b07eb3030fd4d6d35d4353bf37acc407f9957ba4a5e95e5c767dff2642d04`

The typed feed intent was checked against Robinhood's
`eth_getTransactionByHash` response. These values matched exactly:

- sender and canonical router destination;
- selector `swapExactETHForTokens` (`0x7ff36ab5`);
- native transaction value / decoded `amount_in`;
- `amountOutMin`;
- WETH-to-token path;
- recipient;
- deadline.

No transaction was created, signed or submitted during this validation.
