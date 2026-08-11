import type { ConfigActivity, CustomerConfig, FixtureScenario, StoredFixtureState } from "@/lib/customer-config/types";

const baseTargets = [
  {
    id: "kol-scout",
    label: "KOL scout",
    address: "7qMDEMO2tX",
    enabled: true,
    copiedTrades: 18,
    amountOverrideSol: null
  },
  {
    id: "dev-tracker",
    label: "Dev tracker",
    address: "4gNDEMO8aL",
    enabled: true,
    copiedTrades: 6,
    amountOverrideSol: 0.1
  },
  {
    id: "momentum-desk",
    label: "Momentum desk",
    address: "9KrDEMO3pD",
    enabled: true,
    copiedTrades: 11,
    amountOverrideSol: null
  },
  {
    id: "quiet-whale",
    label: "Quiet whale",
    address: "2vCDEMO6mB",
    enabled: false,
    copiedTrades: 2,
    amountOverrideSol: null
  }
];

export const baseConfig: CustomerConfig = {
  revision: 42,
  telegramLinked: true,
  telegramHandle: "@xloupee",
  copyTradingEnabled: true,
  amountPerBuySol: 0.15,
  maxDailyBuys: 20,
  maxPositionSol: 0.6,
  buySlippagePercent: 12,
  sellSlippagePercent: 15,
  priorityFeeSol: 0.0025,
  stopLossEnabled: true,
  stopLossPercent: 18,
  trailingStopEnabled: true,
  trailingStopPercent: 14,
  exitLevels: [
    { id: "tp-1", triggerPercent: 35, sellPercent: 35 },
    { id: "tp-2", triggerPercent: 80, sellPercent: 40 }
  ],
  targets: baseTargets,
  devSnipingEnabled: true,
  blockDevSelling: true,
  alerts: {
    copiedBuy: true,
    copiedSell: true,
    positionWarning: true,
    runtimeFailure: true,
    dailySummary: false
  },
  tradingWallet: {
    address: "7xHDEMOwallet9Q",
    balanceSol: 2.84,
    ready: true
  },
  cashback: {
    referralCode: "HERMES-DEMO",
    earnedSol: 0.184,
    invitedUsers: 7
  }
};

export const baseActivity: ConfigActivity[] = [
  {
    id: "cfg-42",
    type: "configuration",
    title: "Revision 42 became active",
    detail: "Stop loss changed from 20% to 18%.",
    status: "active",
    occurredAt: "Today · 09:42"
  },
  {
    id: "trade-hood",
    type: "trade",
    title: "Copied KOL scout",
    detail: "Bought 0.15 SOL of $HOOD · transaction landed.",
    status: "landed",
    occurredAt: "Today · 08:16"
  },
  {
    id: "trade-bonk",
    type: "trade",
    title: "Protected exit completed",
    detail: "Sold the remaining $BONK position at the configured stop.",
    status: "landed",
    occurredAt: "Yesterday · 21:07"
  },
  {
    id: "cfg-41",
    type: "configuration",
    title: "Momentum desk enabled",
    detail: "Target count changed from 2 to 3.",
    status: "active",
    occurredAt: "Yesterday · 15:31"
  }
];

function cloneConfig(config: CustomerConfig): CustomerConfig {
  return structuredClone(config);
}

export function buildFixture(scenario: FixtureScenario): StoredFixtureState {
  const config = cloneConfig(baseConfig);

  if (scenario === "empty") {
    config.copyTradingEnabled = false;
    config.targets = [];
  }
  if (scenario === "unlinked") {
    config.telegramLinked = false;
    config.telegramHandle = "Not linked";
  }
  if (scenario === "missing-wallet") {
    config.copyTradingEnabled = false;
    config.tradingWallet = null;
  }

  return {
    scenario,
    config,
    activity: structuredClone(baseActivity)
  };
}
