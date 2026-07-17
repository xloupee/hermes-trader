# Hood fresh1 selector and identity evidence (2026-07-16)

Scope: read-only Robinhood Chain mainnet evidence for the two Hood protocol
keys in `paper-session-20260716-pins39-fresh1`. This does not authorize keys,
signing, broadcasting, a canary, or server work.

## Exact detector miss

Transaction
`0xf7298ac6be29ffe53d0bac67be4dd0c1ff3353cd7fecb1be5bd0bc5d5f94ffad`
is a successful direct call to the pinned Hood factory
`0x5fcc1df0dc020cf454e742e9a8ae2554c37a452c` at L2 block
`11717655`, transaction index `15`:

- selector `0xa200dd45` is verified-source
  `buyFor(address token,address recipient,uint256 minTokensOut)`;
- token is `0xec0ae38da2f99a4b666046ca1cb7e8aef47a600d`;
- recipient is `0x0f972050adfc4e4fce6024431eab5b848043605b`;
- the direct signer is the same address in this proof;
- value is `0.061 ETH` and minimum output is
  `20915475452605437937711931` tokens;
- the canonical factory `Trade` event declares `isBuy=true`, the same token
  and recipient/trader, `0.061 ETH`, and
  `23239417169561597708568813` tokens;
- the following ERC-20 transfer is factory to the same recipient for the same
  token amount.

The old observer allowlisted only `buy(address,uint256)` (`0xcce7ec13`), so the
ground-truth `Trade` event was a real detector miss. The fix admits only the
exact 100-byte `buyFor` ABI and rejects zero token, zero recipient, zero value,
zero minimum, non-canonical length, receipt-recipient drift, and receipt-minimum
drift.

## Exact launch identity gap

Transaction
`0xe44731249b53f4c22a7e30d1ed38de7f74adf468cd403ddabeb69b8000c544b7`
is the other fresh1 Hood protocol key. It is canonical direct
`createToken(string,string,string,uint256,bytes32,uint16,uint256)` calldata,
but the previous observer emitted `predicted_token=null`.

The independently verified factory source proves:

```text
effective_salt = keccak256(abi.encodePacked(msg.sender, user_salt))
init_code_hash = keccak256(
  type(HoodToken).creationCode || abi.encode(name, symbol, totalSupply)
)
token = CREATE2(factory, effective_salt, init_code_hash)
```

At reviewed block `11717253`, the already expected-pinned factory runtime is
20,518 bytes with hash
`0x4aa0ce56b5b67d27f2fab59dcb796fa552d10ceafdecb06e088cdd254c92c0fc`.
Its exact embedded `type(HoodToken).creationCode` is bytes
`16829..20465`, length `3,636`, hash
`0xec5e362ebe430cb8be425b477597e8a4394ad22ad9f1d3a3eacfd1ed483aa8dd`.
Appending the fresh1 constructor ABI produces the independently queried
`tokenInitCodeHash`
`0x359579aeef65e65daa6e0b79ead05f2859cffa61b0a87d5ee7a3f9cbb4e5a3d6`.
The local CREATE2 derivation and the factory's fixed-block
`predictTokenAddress` both reproduce the receipt token
`0xec0ae38da2f99a4b666046ca1cb7e8aef47a600d` exactly.

The embedded creation-code bytes are checked at startup against their length
and hash. Canonical ABI decoding and re-encoding are required before local
prediction. No RPC occurs in candidate processing.

## Reproduction

```sh
cast tx <tx> --json --rpc-url https://rpc.mainnet.chain.robinhood.com
cast receipt <tx> --json --rpc-url https://rpc.mainnet.chain.robinhood.com
cast code 0x5fcc1df0dc020cf454e742e9a8ae2554c37a452c \
  --block 11717253 --rpc-url https://rpc.mainnet.chain.robinhood.com
cast call 0x5fcc1df0dc020cf454e742e9a8ae2554c37a452c \
  'tokenInitCodeHash(string,string,uint256)(bytes32)' \
  'MEMES ARE JUST AWESOME' SOMEME 1000000000000000000000000000 \
  --block 11717253 --rpc-url https://rpc.mainnet.chain.robinhood.com
```

Verified source and ABI:
`https://robinhoodchain.blockscout.com/api/v2/smart-contracts/0x5fcc1df0dc020cf454e742e9a8ae2554c37a452c`.
