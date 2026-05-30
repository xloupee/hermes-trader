import assert from "node:assert/strict";
import test from "node:test";
import {
  compareDiscoveryLatency,
  normalizePumpPortalDiscoveryLatencyEvent,
  summarizeDiscoveryLatency
} from "../dist/shredstream-latency.js";

function pumpPortalEvent(overrides) {
  return {
    source: "pumpportal",
    receivedAtMs: 1_000,
    eventType: "buy",
    ...overrides
  };
}

function shredstreamEvent(overrides) {
  return {
    source: "shredstream",
    receivedAtMs: 900,
    eventType: "buy",
    decodeStatus: "decoded",
    ...overrides
  };
}

test("latency comparison matches signature and instruction index first", () => {
  const comparisons = compareDiscoveryLatency({
    pumpPortalEvents: [
      pumpPortalEvent({ signature: "sig", instructionIndex: 1, mint: "wrong", receivedAtMs: 1000 }),
      pumpPortalEvent({ signature: "sig", instructionIndex: 0, mint: "mint", receivedAtMs: 1100 })
    ],
    shredstreamEvents: [shredstreamEvent({ signature: "sig", instructionIndex: 0, mint: "mint", receivedAtMs: 900 })]
  });

  assert.equal(comparisons.length, 1);
  assert.equal(comparisons[0].mint, "mint");
  assert.equal(comparisons[0].shred_minus_pumpportal_ms, -200);
  assert.equal(comparisons[0].source_winner, "shredstream");
});

test("latency comparison falls back to signature and mint", () => {
  const [comparison] = compareDiscoveryLatency({
    pumpPortalEvents: [pumpPortalEvent({ signature: "sig", mint: "mint", receivedAtMs: 1000 })],
    shredstreamEvents: [shredstreamEvent({ signature: "sig", mint: "mint", receivedAtMs: 1200 })]
  });

  assert.equal(comparison.shred_minus_pumpportal_ms, 200);
  assert.equal(comparison.source_winner, "pumpportal");
});

test("latency comparison can match create events by mint within a window", () => {
  const [comparison] = compareDiscoveryLatency({
    pumpPortalEvents: [pumpPortalEvent({ eventType: "create", mint: "mint", receivedAtMs: 1000 })],
    shredstreamEvents: [shredstreamEvent({ eventType: "create", mint: "mint", receivedAtMs: 900 })],
    createWindowMs: 200
  });

  assert.equal(comparison.mint, "mint");
  assert.equal(comparison.source_winner, "shredstream");
});

test("latency comparison does not match create events outside the window", () => {
  const comparisons = compareDiscoveryLatency({
    pumpPortalEvents: [pumpPortalEvent({ eventType: "create", mint: "mint", receivedAtMs: 1000 })],
    shredstreamEvents: [shredstreamEvent({ eventType: "create", mint: "mint", receivedAtMs: 4000 })],
    createWindowMs: 200
  });

  assert.deepEqual(comparisons, []);
});

test("latency summary reports percentiles and missing source counts", () => {
  const pumpPortalEvents = [
    pumpPortalEvent({ signature: "a", instructionIndex: 0, mint: "a", receivedAtMs: 1000 }),
    pumpPortalEvent({ signature: "b", instructionIndex: 0, mint: "b", receivedAtMs: 1000 }),
    pumpPortalEvent({ signature: "missing-shred", instructionIndex: 0, mint: "c", receivedAtMs: 1000 })
  ];
  const shredstreamEvents = [
    shredstreamEvent({ signature: "a", instructionIndex: 0, mint: "a", receivedAtMs: 900 }),
    shredstreamEvent({ signature: "b", instructionIndex: 0, mint: "b", receivedAtMs: 1300 }),
    shredstreamEvent({ signature: "missing-pump", instructionIndex: 0, mint: "d", receivedAtMs: 800 })
  ];
  const comparisons = compareDiscoveryLatency({ pumpPortalEvents, shredstreamEvents });
  const summary = summarizeDiscoveryLatency({ pumpPortalEvents, shredstreamEvents, comparisons });

  assert.deepEqual(summary, {
    matchedCount: 2,
    missingPumpPortalCount: 1,
    missingShredstreamCount: 1,
    shredWins: 1,
    pumpPortalWins: 1,
    ties: 0,
    p50DeltaMs: -100,
    p90DeltaMs: 300,
    p99DeltaMs: 300
  });
});

test("normalizes PumpPortal discovery events for side-by-side comparison", () => {
  const event = normalizePumpPortalDiscoveryLatencyEvent(
    {
      txType: "create",
      mint: "mint",
      signature: "sig",
      slot: "123",
      instructionIndex: "4",
      programId: "program"
    },
    1500
  );

  assert.deepEqual(event, {
    source: "pumpportal",
    signature: "sig",
    instructionIndex: 4,
    mint: "mint",
    receivedAtMs: 1500,
    slot: 123,
    programId: "program",
    eventType: "create"
  });
});

test("ignores PumpPortal records that are not discovery events", () => {
  assert.equal(normalizePumpPortalDiscoveryLatencyEvent({ txType: "subscribe" }, 1500), null);
  assert.equal(normalizePumpPortalDiscoveryLatencyEvent({}, 1500), null);
});
