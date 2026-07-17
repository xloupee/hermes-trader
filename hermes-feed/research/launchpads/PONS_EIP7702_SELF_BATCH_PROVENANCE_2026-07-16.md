# Pons EIP-7702 self-batch paper profile

This review authorizes one paper-only observation and receipt-quote profile. It does not
authorize generic `0x3f707e6b` dispatch, arbitrary EIP-7702 accounts, sibling call shapes,
predictive identity admission, signing, broadcast, execution, deployment, or canaries.

## Provenance boundary

- Robinhood Chain ID: `4663`.
- Proof transaction: `0x7a13c94f90ddaa7d35d639f046f30a44d1d9b5fe449550fd0b75e5e65a0fb4c6`.
- Type/block/index: type `4`, L2 `11777530`, block
  `0xd8cda07d851127f7c500e598aaa63e8ec8a3d6b3bae39556d2e7c6ed92801fd6`, index `7`.
- Signer, authorization authority, and outer self-target:
  `0xfB3538B3Fac2cc5ffC582446c55875A889AbD146`.
- Single authorization: chain `4663`, nonce `15`, implementation
  `0xdC44136e7CA3509A73Fc6C22b6a6bd302BF9A1E2`.
- Account designator: `0xef0100dc44136e7ca3509a73fc6c22b6a6bd302bf9a1e2`; runtime hash
  `0x9bdfa4cdd2727209e60a7bbb51630848fd4abafc4f612d59fde7018262fa23f3`.
- Delegated implementation runtime hash:
  `0x6d7379e6220b87ceeade4a4e069c6a5ca4636fc228a0c948a0c87177860f3baa`.
- Implementation deployment transaction independently reviewed:
  `0xf9e2b8d0c51a2357469ca8d4b06f2c4abc6d6456f843550e1f0ce0152c25a49e`.

Expected commitments live in `config/launchpad-expected-pins.production.json`. Fresh startup
observations remain separate evidence and must supply both the 23-byte account designator and
the delegated implementation runtime. Either missing half fails startup.

## Exact reviewed call profile

The outer call is the canonical ABI encoding of `execute((address,uint256,bytes)[])`, with zero
outer transaction value and exactly two ordered calls:

1. Auxiliary target `0x83cAb64494cFf66cE1c331fA9224692bDEcE5ABb`, value
   `538332961881668`, and empty calldata. This is named only as the reviewed auxiliary call; no
   generic fee semantics are inferred.
2. Current Pons factory `0x0c37a24F5D23A486FA692d1500881d698B1F77a4`, value
   `60500000000000000`, and canonical `launchToken` selector `0x686399cb` with config IDs zero
   and developer wallet equal to the reviewed account.

The global registry contains only the unwrapped dispatch key `(current Pons factory,
0x686399cb, Eip7702SelfBatch)`. It contains no self-target/outer-selector key.

## Receipt and paper result

The proof receipt independently establishes token
`0x331a3c242517127cEC8ba5d974b2Cb07b9050363`, pool
`0x05A92349a7Af8456474B3Ef3ca7dDa859677535A`, position `184384`, fee `10000`, spacing
`200`, and `60000000000000000` wei initial buy. Receipt admission requires inner value to equal
the `500000000000000` wei Pons launch fee plus `TokenLaunched.initialBuyAmount`.

Entry and immediate full-position-exit quotes are reconstructed independently from receipt-end
V3 state. Wrapper provenance is retained in the observation, reconciliation request, and quote.
Every result remains `execution_eligible=false` and `broadcast=false`.
