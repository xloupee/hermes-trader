export interface CopyTradeExecutionModeConfig {
  copyTradeEnabled: boolean;
  copyTradeDryRun: boolean;
}

export function copyTradeLiveExecutionEnabled(config: CopyTradeExecutionModeConfig): boolean {
  return config.copyTradeEnabled && !config.copyTradeDryRun;
}

export function copyTradeLiveExecutionBlockedReason(config: CopyTradeExecutionModeConfig): string | null {
  if (!config.copyTradeEnabled) {
    return "COPY_TRADE_ENABLED is not true";
  }

  if (config.copyTradeDryRun) {
    return "COPY_TRADE_DRY_RUN is enabled";
  }

  return null;
}

export function copyTradeExecutionStateLabel(config: CopyTradeExecutionModeConfig): string {
  if (copyTradeLiveExecutionEnabled(config)) {
    return "LIVE";
  }

  if (!config.copyTradeEnabled) {
    return config.copyTradeDryRun ? "DISABLED + DRY RUN" : "DISABLED";
  }

  return "DRY RUN";
}

export function formatCopyTradeExecutionStateLog(config: CopyTradeExecutionModeConfig): string {
  return [
    `Copy trade execution state: ${copyTradeExecutionStateLabel(config)}`,
    `COPY_TRADE_ENABLED=${config.copyTradeEnabled ? "true" : "false"}`,
    `COPY_TRADE_DRY_RUN=${config.copyTradeDryRun ? "true" : "false"}`,
    `livePumpPortalSubmissions=${copyTradeLiveExecutionEnabled(config) ? "allowed" : "blocked"}`
  ].join(" | ");
}
