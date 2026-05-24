export interface CopyTradeExecutionModeConfig {
  copyTradeEnabled: boolean;
  copyTradeDryRun: boolean;
  copyTradeEmergencyStopped?: boolean;
}

export function copyTradeLiveExecutionEnabled(config: CopyTradeExecutionModeConfig): boolean {
  return config.copyTradeEnabled && !config.copyTradeDryRun && !config.copyTradeEmergencyStopped;
}

export function copyTradeLiveExecutionBlockedReason(config: CopyTradeExecutionModeConfig): string | null {
  if (config.copyTradeEmergencyStopped) {
    return "copy trade emergency stop is active";
  }

  if (!config.copyTradeEnabled) {
    return "COPY_TRADE_ENABLED is not true";
  }

  if (config.copyTradeDryRun) {
    return "COPY_TRADE_DRY_RUN is enabled";
  }

  return null;
}

export function copyTradeExecutionStateLabel(config: CopyTradeExecutionModeConfig): string {
  if (config.copyTradeEmergencyStopped) {
    return "EMERGENCY STOPPED";
  }

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
    `emergencyStop=${config.copyTradeEmergencyStopped ? "active" : "inactive"}`,
    `livePumpPortalSubmissions=${copyTradeLiveExecutionEnabled(config) ? "allowed" : "blocked"}`
  ].join(" | ");
}
