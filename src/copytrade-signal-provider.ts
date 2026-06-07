export const COPY_TRADE_SIGNAL_PROVIDERS = ["pumpportal", "geyser", "parallel"] as const;

export type CopyTradeSignalProvider = (typeof COPY_TRADE_SIGNAL_PROVIDERS)[number];
export type WalletTradeSignalSource = "pumpportal" | "geyser" | "helius";

export function normalizeCopyTradeSignalProvider(value: string | null | undefined): CopyTradeSignalProvider | null {
  const normalized = value?.trim().toLowerCase();
  if (!normalized) {
    return null;
  }

  if (normalized === "shredstream") {
    return "geyser";
  }

  return COPY_TRADE_SIGNAL_PROVIDERS.includes(normalized as CopyTradeSignalProvider)
    ? (normalized as CopyTradeSignalProvider)
    : null;
}

export function parseCopyTradeSignalProvider(
  value: string | null | undefined,
  fallback: CopyTradeSignalProvider = "pumpportal"
): CopyTradeSignalProvider {
  return normalizeCopyTradeSignalProvider(value) || fallback;
}

export function copyTradeSignalProviderConfigError(value: string | null | undefined): string | null {
  if (!value?.trim()) {
    return null;
  }

  return normalizeCopyTradeSignalProvider(value)
    ? null
    : `unsupported copy trade signal provider "${value}"; expected ${COPY_TRADE_SIGNAL_PROVIDERS.join(", ")}`;
}

export function copyTradeSignalSourceForWalletTradeProvider(provider: string | null | undefined): WalletTradeSignalSource | null {
  const normalized = provider?.trim().toLowerCase();

  if (normalized === "pumpportal") {
    return "pumpportal";
  }

  if (normalized === "geyser" || normalized === "yellowstone") {
    return "geyser";
  }

  if (normalized === "helius") {
    return "helius";
  }

  return null;
}

export function copyTradeSignalProviderAllows({
  configured,
  source
}: {
  configured: CopyTradeSignalProvider;
  source: WalletTradeSignalSource;
}): boolean {
  if (configured === "parallel") {
    return source === "pumpportal" || source === "geyser";
  }

  return configured === source;
}
