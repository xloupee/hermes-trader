import assert from "node:assert/strict";
import test from "node:test";
import {
  copyTradeSignalProviderAllows,
  copyTradeSignalProviderConfigError,
  copyTradeSignalSourceForWalletTradeProvider,
  parseCopyTradeSignalProvider
} from "../dist/copytrade-signal-provider.js";

test("copy trade signal provider parsing defaults to PumpPortal", () => {
  assert.equal(parseCopyTradeSignalProvider(null), "pumpportal");
  assert.equal(parseCopyTradeSignalProvider(" GEYSER "), "geyser");
  assert.equal(parseCopyTradeSignalProvider("shredstream"), "geyser");
  assert.equal(parseCopyTradeSignalProvider("parallel"), "parallel");
  assert.equal(parseCopyTradeSignalProvider("nope"), "pumpportal");
  assert.match(copyTradeSignalProviderConfigError("nope") || "", /unsupported copy trade signal provider/);
  assert.equal(copyTradeSignalProviderConfigError("shredstream"), null);
  assert.equal(copyTradeSignalProviderConfigError("pumpportal"), null);
});

test("copy trade signal provider gates PumpPortal and Geyser triggers", () => {
  assert.equal(copyTradeSignalProviderAllows({ configured: "pumpportal", source: "pumpportal" }), true);
  assert.equal(copyTradeSignalProviderAllows({ configured: "pumpportal", source: "geyser" }), false);
  assert.equal(copyTradeSignalProviderAllows({ configured: "geyser", source: "pumpportal" }), false);
  assert.equal(copyTradeSignalProviderAllows({ configured: "geyser", source: "geyser" }), true);
  assert.equal(copyTradeSignalProviderAllows({ configured: "parallel", source: "pumpportal" }), true);
  assert.equal(copyTradeSignalProviderAllows({ configured: "parallel", source: "geyser" }), true);
  assert.equal(copyTradeSignalProviderAllows({ configured: "parallel", source: "helius" }), false);
});

test("Yellowstone wallet trades are treated as Geyser signals", () => {
  assert.equal(copyTradeSignalSourceForWalletTradeProvider("pumpportal"), "pumpportal");
  assert.equal(copyTradeSignalSourceForWalletTradeProvider("yellowstone"), "geyser");
  assert.equal(copyTradeSignalSourceForWalletTradeProvider("geyser"), "geyser");
  assert.equal(copyTradeSignalSourceForWalletTradeProvider("helius"), "helius");
});
