const number = (value, suffix = "") => `${Number(value).toLocaleString("en-US", { maximumFractionDigits: 4 })}${suffix}`;
const state = (value) => value ? "Enabled" : "Paused";

export function createConfigDiff(before, after) {
  const changes = [];
  const add = (id, group, label, previous, next, warning) => {
    if (previous === next) return;
    changes.push({ id, group, label, before: previous, after: next, ...(warning ? { warning } : {}) });
  };

  add("copy-trading", "Copy trading", "Copy trading", state(before.copyTradingEnabled), state(after.copyTradingEnabled));
  add(
    "amount",
    "Copy trading",
    "Amount per buy",
    number(before.amountPerBuySol, " SOL"),
    number(after.amountPerBuySol, " SOL"),
    after.amountPerBuySol > before.amountPerBuySol ? "Higher exposure on every future copied buy." : undefined
  );
  add("daily-buys", "Risk and fees", "Maximum daily buys", String(before.maxDailyBuys), String(after.maxDailyBuys), after.maxDailyBuys > before.maxDailyBuys ? "Allows more buys per day." : undefined);
  add("max-position", "Risk and fees", "Maximum position", number(before.maxPositionSol, " SOL"), number(after.maxPositionSol, " SOL"), after.maxPositionSol > before.maxPositionSol ? "Raises the maximum position size." : undefined);
  add("buy-slippage", "Risk and fees", "Buy slippage", number(before.buySlippagePercent, "%"), number(after.buySlippagePercent, "%"), after.buySlippagePercent > before.buySlippagePercent ? "Accepts a wider execution range." : undefined);
  add("sell-slippage", "Risk and fees", "Sell slippage", number(before.sellSlippagePercent, "%"), number(after.sellSlippagePercent, "%"), after.sellSlippagePercent > before.sellSlippagePercent ? "Accepts a wider execution range." : undefined);
  add("priority-fee", "Risk and fees", "Priority fee", number(before.priorityFeeSol, " SOL"), number(after.priorityFeeSol, " SOL"), after.priorityFeeSol > before.priorityFeeSol ? "Increases the maximum fee per transaction." : undefined);
  add("stop-enabled", "Exit strategy", "Stop loss", state(before.stopLossEnabled), state(after.stopLossEnabled), before.stopLossEnabled && !after.stopLossEnabled ? "Removes downside protection." : undefined);
  add("stop-percent", "Exit strategy", "Stop-loss trigger", number(before.stopLossPercent, "% below entry"), number(after.stopLossPercent, "% below entry"));
  add("trailing-enabled", "Exit strategy", "Trailing stop", state(before.trailingStopEnabled), state(after.trailingStopEnabled));
  add("trailing-percent", "Exit strategy", "Trailing distance", number(before.trailingStopPercent, "%"), number(after.trailingStopPercent, "%"));
  add("dev-sniping", "Dev sniping", "Dev sniping", state(before.devSnipingEnabled), state(after.devSnipingEnabled));
  add("block-dev-selling", "Dev sniping", "Block while dev sells", state(before.blockDevSelling), state(after.blockDevSelling));

  const beforeTargets = new Map(before.targets.map((target) => [target.id, target]));
  for (const target of after.targets) {
    const previous = beforeTargets.get(target.id);
    if (!previous) {
      changes.push({ id: `target-${target.id}`, group: "Target wallets", label: target.label, before: "Not tracked", after: "Active", warning: "Adds a wallet that can trigger future buys." });
      continue;
    }
    add(`target-${target.id}`, "Target wallets", target.label, state(previous.enabled), state(target.enabled), !previous.enabled && target.enabled ? "Adds another source of future copied buys." : undefined);
  }

  for (const key of Object.keys(after.alerts)) {
    add(`alert-${key}`, "Alerts", key.replace(/([A-Z])/g, " $1").replace(/^./, (letter) => letter.toUpperCase()), state(before.alerts[key]), state(after.alerts[key]));
  }

  return changes;
}

export function plannedExposure(config) {
  const activeTargets = config.targets.filter((target) => target.enabled);
  return activeTargets.reduce((total, target) => total + (target.amountOverrideSol ?? config.amountPerBuySol), 0);
}
