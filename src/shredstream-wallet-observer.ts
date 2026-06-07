import { normalizeShredstreamTransaction, rawPumpDiscoveryEventToWalletTrade } from "./shredstream-decoder.js";
import { createShredstreamSourceFromEnv, type ShredstreamSource } from "./shredstream-source.js";
import { errorMessage, type ExplorerConfig, type WalletTradeData, type WatchedWallet } from "./types.js";

export interface ShredstreamWalletObserver {
  start: () => void;
  stop: () => void;
  setWallets: (wallets: WatchedWallet[]) => void;
}

export interface ShredstreamWalletObserverOptions {
  enabled: boolean;
  source?: ShredstreamSource;
  wallets: WatchedWallet[];
  explorer: ExplorerConfig;
  statsIntervalMs?: number;
  isDiagnosticWallet?: (wallet: WatchedWallet) => boolean;
  onTrade: (trade: WalletTradeData, timing: { receivedAtMs: number; normalizedAtMs: number }) => void | Promise<void>;
  onStatus?: (message: string) => void;
  onError?: (error: Error) => void;
}

interface ShredstreamWalletObserverStats {
  startedAtMs: number;
  recordsRead: number;
  parseErrors: number;
  pumpEvents: number;
  decodedWalletCandidates: number;
  watchedWalletMatches: number;
  diagnosticWalletMatches: number;
  realWalletMatches: number;
  ambiguousWalletCandidates: number;
  duplicateWalletCandidates: number;
  tradesEmitted: number;
  diagnosticTradesEmitted: number;
  realTradesEmitted: number;
}

export function createShredstreamWalletObserver({
  enabled,
  source = createShredstreamSourceFromEnv(),
  wallets,
  explorer,
  statsIntervalMs = 60_000,
  isDiagnosticWallet = () => false,
  onTrade,
  onStatus = () => {},
  onError = () => {}
}: ShredstreamWalletObserverOptions): ShredstreamWalletObserver {
  let activeWallets = uniqueWallets(wallets);
  let abortController: AbortController | null = null;
  let running = false;
  let loopRunning = false;

  async function observeLoop(signal: AbortSignal): Promise<void> {
    if (loopRunning) {
      return;
    }

    loopRunning = true;
    onStatus(`ShredStream wallet observer started: source=${source.describe()} wallets=${activeWallets.size} realWallets=${realWalletCount()} diagnosticWallets=${diagnosticWalletCount()}`);
    const stats: ShredstreamWalletObserverStats = {
      startedAtMs: Date.now(),
      recordsRead: 0,
      parseErrors: 0,
      pumpEvents: 0,
      decodedWalletCandidates: 0,
      watchedWalletMatches: 0,
      diagnosticWalletMatches: 0,
      realWalletMatches: 0,
      ambiguousWalletCandidates: 0,
      duplicateWalletCandidates: 0,
      tradesEmitted: 0,
      diagnosticTradesEmitted: 0,
      realTradesEmitted: 0
    };
    let nextStatsAtMs = stats.startedAtMs + statsIntervalMs;

    function maybeEmitStats(nowMs = Date.now(), force = false): void {
      if (!force && (!Number.isFinite(statsIntervalMs) || statsIntervalMs <= 0 || nowMs < nextStatsAtMs)) {
        return;
      }

      nextStatsAtMs = nowMs + statsIntervalMs;
      onStatus(`ShredStream wallet observer stats: ${JSON.stringify({
        source: source.describe(),
        wallets: activeWallets.size,
        realWallets: realWalletCount(),
        diagnosticWallets: diagnosticWalletCount(),
        uptimeMs: nowMs - stats.startedAtMs,
        recordsRead: stats.recordsRead,
        parseErrors: stats.parseErrors,
        pumpEvents: stats.pumpEvents,
        decodedWalletCandidates: stats.decodedWalletCandidates,
        watchedWalletMatches: stats.watchedWalletMatches,
        diagnosticWalletMatches: stats.diagnosticWalletMatches,
        realWalletMatches: stats.realWalletMatches,
        ambiguousWalletCandidates: stats.ambiguousWalletCandidates,
        duplicateWalletCandidates: stats.duplicateWalletCandidates,
        tradesEmitted: stats.tradesEmitted,
        diagnosticTradesEmitted: stats.diagnosticTradesEmitted,
        realTradesEmitted: stats.realTradesEmitted
      })}`);
    }

    try {
      for await (const record of source.readRecords({ signal })) {
        if (signal.aborted) {
          return;
        }

        stats.recordsRead += 1;
        if (!record.transaction) {
          if (record.parseError) {
            stats.parseErrors += 1;
          }
          maybeEmitStats();
          continue;
        }

        const events = normalizeShredstreamTransaction(record.transaction);
        const ambiguousTradeKeys = ambiguousWalletTradeKeys(events);
        const emittedTradeKeys = new Set<string>();
        for (const event of events) {
          stats.pumpEvents += 1;
          if (event.decodeStatus !== "decoded" || (event.eventType !== "buy" && event.eventType !== "sell")) {
            continue;
          }

          stats.decodedWalletCandidates += 1;
          const wallet = event.trader ? activeWallets.get(event.trader) : undefined;
          if (!wallet) {
            continue;
          }

          stats.watchedWalletMatches += 1;
          const diagnostic = isDiagnosticWallet(wallet);
          if (diagnostic) {
            stats.diagnosticWalletMatches += 1;
          } else {
            stats.realWalletMatches += 1;
          }
          const tradeKey = event.mint ? `${event.signature}|${event.trader}|${event.mint}|${event.eventType}` : null;
          const ambiguityKey = event.mint ? `${event.signature}|${event.trader}|${event.mint}` : null;
          if (ambiguityKey && ambiguousTradeKeys.has(ambiguityKey)) {
            stats.ambiguousWalletCandidates += 1;
            continue;
          }

          if (tradeKey && emittedTradeKeys.has(tradeKey)) {
            stats.duplicateWalletCandidates += 1;
            continue;
          }

          const normalizedAtMs = Date.now();
          const trade = rawPumpDiscoveryEventToWalletTrade({ event, wallet, explorer });
          if (!trade) {
            continue;
          }

          if (tradeKey) {
            emittedTradeKeys.add(tradeKey);
          }
          stats.tradesEmitted += 1;
          if (diagnostic) {
            stats.diagnosticTradesEmitted += 1;
          } else {
            stats.realTradesEmitted += 1;
          }
          Promise.resolve(onTrade(trade, { receivedAtMs: event.receivedAtMs, normalizedAtMs })).catch((error: unknown) =>
            onError(error instanceof Error ? error : new Error(errorMessage(error)))
          );
        }
        maybeEmitStats();
      }
      maybeEmitStats(Date.now(), true);
    } catch (error) {
      if (!signal.aborted) {
        onError(error instanceof Error ? error : new Error(errorMessage(error)));
      }
    } finally {
      loopRunning = false;
      if (running && enabled && !signal.aborted) {
        setTimeout(() => startLoop(), 1000);
      }
    }
  }

  function startLoop(): void {
    if (!running || !enabled || loopRunning || activeWallets.size === 0) {
      if (running && enabled && activeWallets.size === 0) {
        onStatus("ShredStream wallet observer waiting for watched wallets");
      }
      return;
    }

    abortController = new AbortController();
    void observeLoop(abortController.signal);
  }

  return {
    start() {
      running = true;
      if (!enabled) {
        onStatus("ShredStream wallet observer disabled; set SHREDSTREAM_WALLET_OBSERVER_ENABLED=true to compare watched wallets.");
        return;
      }
      startLoop();
    },
    stop() {
      running = false;
      abortController?.abort();
      abortController = null;
    },
    setWallets(wallets: WatchedWallet[]) {
      activeWallets = uniqueWallets(wallets);
      if (running && enabled && !loopRunning) {
        startLoop();
      }
    }
  };

  function diagnosticWalletCount(): number {
    return [...activeWallets.values()].filter(isDiagnosticWallet).length;
  }

  function realWalletCount(): number {
    return activeWallets.size - diagnosticWalletCount();
  }
}

function ambiguousWalletTradeKeys(events: ReturnType<typeof normalizeShredstreamTransaction>): Set<string> {
  const actionsByKey = new Map<string, Set<string>>();

  for (const event of events) {
    if (
      event.decodeStatus !== "decoded" ||
      (event.eventType !== "buy" && event.eventType !== "sell") ||
      !event.signature ||
      !event.trader ||
      !event.mint
    ) {
      continue;
    }

    const key = `${event.signature}|${event.trader}|${event.mint}`;
    const actions = actionsByKey.get(key) || new Set<string>();
    actions.add(event.eventType);
    actionsByKey.set(key, actions);
  }

  return new Set(
    [...actionsByKey.entries()]
      .filter(([, actions]) => actions.size > 1)
      .map(([key]) => key)
  );
}

function uniqueWallets(wallets: WatchedWallet[]): Map<string, WatchedWallet> {
  return new Map(
    wallets
      .filter((wallet) => wallet.address)
      .map((wallet) => [wallet.address, wallet])
  );
}
