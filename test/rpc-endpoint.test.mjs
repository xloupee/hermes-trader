import assert from "node:assert/strict";
import test from "node:test";
import { formatSolanaRpcEndpointLog, redactSolanaRpcUrl } from "../dist/rpc-endpoint.js";

test("Solana RPC endpoint logging preserves plain VA URL", () => {
  assert.equal(redactSolanaRpcUrl("http://va.pixellabz.io/"), "http://va.pixellabz.io/");
  assert.equal(
    formatSolanaRpcEndpointLog("http://va.pixellabz.io/"),
    "Solana RPC endpoint | url=http://va.pixellabz.io/"
  );
});

test("Solana RPC endpoint logging redacts API keys and credentials", () => {
  assert.equal(
    redactSolanaRpcUrl("https://user:pass@mainnet.helius-rpc.com/?api-key=secret-value&commitment=confirmed"),
    "https://redacted:redacted@mainnet.helius-rpc.com/?api-key=redacted&commitment=confirmed"
  );

  assert.equal(
    redactSolanaRpcUrl("not-a-url?api-key=secret-value&token=abc"),
    "not-a-url?api-key=redacted&token=redacted"
  );
});
