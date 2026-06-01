import { isDirectTradeExecutionProvider } from "./trade-execution.js";
import type { TradeExecutionProvider } from "./trade-execution.js";

export interface DirectCanaryGateConfig {
  directExecutionEnabled: boolean;
  directExecutionLiveEnabled: boolean;
  directExecutionBuildOnly: boolean;
  directExecutionSimulateOnly: boolean;
  directExecutionCanaryChatIds: string[];
  directExecutionCanaryWallets: string[];
}

export function directCanaryBlockedReason({
  provider,
  tradingWalletPublicKey,
  config
}: {
  provider: TradeExecutionProvider;
  chatId: string;
  tradingWalletPublicKey: string;
  config: DirectCanaryGateConfig;
}): string | null {
  if (!isDirectTradeExecutionProvider(provider)) {
    return null;
  }

  const directExecutionModeEnabled =
    config.directExecutionEnabled &&
    (config.directExecutionLiveEnabled || config.directExecutionBuildOnly || config.directExecutionSimulateOnly);

  if (!directExecutionModeEnabled) {
    return null;
  }

  if (
    config.directExecutionCanaryWallets.length > 0 &&
    !config.directExecutionCanaryWallets.includes(tradingWalletPublicKey)
  ) {
    return `trading wallet ${tradingWalletPublicKey} is not in DIRECT_EXECUTION_CANARY_WALLETS`;
  }

  return null;
}
