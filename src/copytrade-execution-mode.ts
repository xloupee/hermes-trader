export interface CopyTradeExecutionModeConfig {
  copyTradeEnabled: boolean;
  copyTradeDryRun: boolean;
  copyTradeEmergencyStopped?: boolean;
  copyTradeExecutionProvider?: "pumpportal-lightning" | "direct-pump" | "direct-pumpswap" | "direct-auto";
  directExecutionEnabled?: boolean;
  directExecutionLiveEnabled?: boolean;
  directExecutionBuildOnly?: boolean;
  directExecutionSimulateOnly?: boolean;
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
  const provider = config.copyTradeExecutionProvider || "pumpportal-lightning";
  const directProvider = provider === "direct-pump" || provider === "direct-pumpswap" || provider === "direct-auto";

  return [
    `Copy trade execution state: ${copyTradeExecutionStateLabel(config)}`,
    `executionProvider=${provider}`,
    `COPY_TRADE_ENABLED=${config.copyTradeEnabled ? "true" : "false"}`,
    `COPY_TRADE_DRY_RUN=${config.copyTradeDryRun ? "true" : "false"}`,
    `emergencyStop=${config.copyTradeEmergencyStopped ? "active" : "inactive"}`,
    `livePumpPortalSubmissions=${!directProvider && copyTradeLiveExecutionEnabled(config) ? "allowed" : "blocked"}`,
    `directExecution=${config.directExecutionEnabled ? "enabled" : "disabled"}`,
    `directLive=${config.directExecutionLiveEnabled ? "enabled" : "disabled"}`,
    `directMode=${config.directExecutionBuildOnly ? "build-only" : config.directExecutionSimulateOnly ? "simulate-only" : "send"}`
  ].join(" | ");
}
