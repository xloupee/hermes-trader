"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { Activity, CirclePause, CirclePlay, Filter, LogOut, RefreshCcw, Search, TimerReset } from "lucide-react";
import { DetailList, MetricGrid, MetricStrip } from "@/components/benchmark-primitives";
import { amount, duration, ms, queryString, short, sol, us } from "@/lib/benchmark-format";
import {
  actionTone,
  autoSellStatus,
  chainError,
  copyAckMs,
  copyStatus,
  crossSlotLanding,
  decisionTone,
  executionLocalDetectUs,
  firstNumber,
  positionSummary,
  positionTone,
  sameSlotTxsAfterWallet,
  slotDelta,
  subtractMs,
  txsAfterWallet
} from "@/lib/benchmark-position";
import { benchmarkLatencyCells, durationWithFallback } from "@/lib/benchmark-row-display";
import type { BenchmarkRow } from "@/lib/benchmark-rows";
import { metricStats } from "@/lib/benchmark-stats";
import type { LocalExecutionReport } from "@/lib/local-executions";
import { useAutoRefreshQuery } from "@/lib/use-auto-refresh-query";

interface ExecutionSummary {
  total: number;
  sent: number;
  landed: number;
  failedOnChain: number;
  autoSellLanded: number;
  autoSellFailedOnChain: number;
  skipped: number;
  errors: number;
  avgSignatureMs: number | null;
  avgSlotDelta: number | null;
  totalGrossSpendSol: number | null;
  totalExtraSpendSol: number | null;
}

interface BenchmarkRowsResponse {
  rows: BenchmarkRow[];
  summary: ExecutionSummary;
}

interface BenchmarkRowDetailResponse {
  row: BenchmarkRow;
}

export function SignalFeed({ adminEmail }: { adminEmail: string | null }) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [selectedDetail, setSelectedDetail] = useState<BenchmarkRow | null>(null);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [filters, setFilters] = useState({
    since: "24h",
    targetWallet: "",
    mint: "",
    action: "",
    route: "",
    provider: "",
    maxLagMs: ""
  });

  const params = useMemo(() => queryString({ ...filters, limit: "50" }), [filters]);
  const fetcher = useCallback(async (): Promise<BenchmarkRowsResponse> => {
    const response = await fetch(`/api/signals/benchmark-rows?${params}`);
    if (!response.ok) {
      throw new Error("Could not load benchmark rows");
    }
    return await response.json() as BenchmarkRowsResponse;
  }, [params]);

  const { data, loading, error, paused, autoPaused, setPaused, lastUpdated, refresh } = useAutoRefreshQuery(fetcher, { intervalMs: 10000 });
  const rows = data?.rows ?? [];
  const executionSummary = data?.summary ?? null;
  const selectedBase = rows.find((row) => row.id === selectedId) ?? rows[0] ?? null;
  const selectedBaseId = selectedBase?.id ?? null;
  const selectedBaseSignalId = selectedBase?.signalObservationId ?? null;
  const selected = selectedDetail?.id === selectedBase?.id ? selectedDetail : selectedBase;
  const selectedExecution = selected?.execution ?? null;
  const selectedSignal = selected?.signal ?? null;
  const executions = useMemo(
    () => rows.map((row) => row.execution).filter((execution): execution is LocalExecutionReport => Boolean(execution)),
    [rows]
  );

  const executionBenchmarkStats = useMemo(() => ({
    total: metricStats(executions.map((row) => row.observedToSignatureReturnedMs)),
    build: metricStats(executions.map((row) => row.observedToSignedMs)),
    submit: metricStats(executions.map((row) => row.observedToSendSubmittedMs)),
    ack: metricStats(executions.map(copyAckMs))
  }), [executions]);

  useEffect(() => {
    if (!selectedBaseId) {
      setSelectedDetail(null);
      setDetailError(null);
      return;
    }

    let cancelled = false;
    const detailParams = queryString({
      rowId: selectedBaseId,
      signalId: selectedBaseSignalId ? String(selectedBaseSignalId) : ""
    });
    setDetailError(null);
    fetch(`/api/signals/benchmark-rows/detail?${detailParams}`)
      .then(async (response) => {
        if (!response.ok) {
          throw new Error("Could not load row diagnostics");
        }
        return await response.json() as BenchmarkRowDetailResponse;
      })
      .then((detail) => {
        if (!cancelled) {
          setSelectedDetail(detail.row);
        }
      })
      .catch((detailLoadError) => {
        if (!cancelled) {
          setSelectedDetail(null);
          setDetailError(detailLoadError instanceof Error ? detailLoadError.message : String(detailLoadError));
        }
      });

    return () => {
      cancelled = true;
    };
  }, [lastUpdated, selectedBaseId, selectedBaseSignalId]);

  async function signOut() {
    await fetch("/api/auth/logout", { method: "POST" });
    window.location.assign("/login");
  }

  return (
    <main className="ops-shell">
      <header className="topbar">
        <div>
          <p className="eyebrow">Copy Benchmark</p>
          <h1>Signal + Execution</h1>
        </div>
        <div className="topbar-actions">
          <span className="admin-pill">{adminEmail || "admin"}</span>
          <button className="icon-button" onClick={() => refresh()} title="Refresh" type="button">
            <RefreshCcw size={17} />
          </button>
          <button className="icon-button" onClick={() => setPaused((value) => !value)} title={paused || autoPaused ? "Resume auto-refresh" : "Pause auto-refresh"} type="button">
            {paused || autoPaused ? <CirclePlay size={17} /> : <CirclePause size={17} />}
          </button>
          <button className="icon-button" onClick={signOut} title="Sign out" type="button">
            <LogOut size={17} />
          </button>
        </div>
      </header>

      <MetricStrip items={[
        { label: "Trades", value: rows.length },
        { label: "Copy sig p50/p90", value: `${ms(executionBenchmarkStats.total.p50)} / ${ms(executionBenchmarkStats.total.p90)}` },
        { label: "Build p50/p90", value: `${ms(executionBenchmarkStats.build.p50)} / ${ms(executionBenchmarkStats.build.p90)}` },
        { label: "Submit p50/p90", value: `${ms(executionBenchmarkStats.submit.p50)} / ${ms(executionBenchmarkStats.submit.p90)}` },
        { label: "Ack p50/p90", value: `${ms(executionBenchmarkStats.ack.p50)} / ${ms(executionBenchmarkStats.ack.p90)}` },
        { label: "Copy landed/sent", value: `${executionSummary?.landed ?? 0} / ${executionSummary?.sent ?? 0}` },
        { label: "Sell landed/failed", value: `${executionSummary?.autoSellLanded ?? 0} / ${executionSummary?.autoSellFailedOnChain ?? 0}` }
      ]} />

      <section className="filters" aria-label="Signal filters">
        <Filter size={16} />
        <input value={filters.since} onChange={(event) => setFilters({ ...filters, since: event.target.value })} aria-label="Since" />
        <input value={filters.targetWallet} onChange={(event) => setFilters({ ...filters, targetWallet: event.target.value })} placeholder="wallet" aria-label="Target wallet" />
        <input value={filters.mint} onChange={(event) => setFilters({ ...filters, mint: event.target.value })} placeholder="CA" aria-label="Contract address" />
        <select value={filters.action} onChange={(event) => setFilters({ ...filters, action: event.target.value })} aria-label="Action">
          <option value="">action</option>
          <option value="buy">buy</option>
          <option value="sell">sell</option>
        </select>
        <select value={filters.route} onChange={(event) => setFilters({ ...filters, route: event.target.value })} aria-label="Route">
          <option value="">route</option>
          <option value="pump">pump</option>
          <option value="pump-amm">pump-amm</option>
          <option value="flashx-pump">flashx-pump</option>
        </select>
        <input value={filters.provider} onChange={(event) => setFilters({ ...filters, provider: event.target.value })} placeholder="provider" aria-label="Provider" />
        <input value={filters.maxLagMs} onChange={(event) => setFilters({ ...filters, maxLagMs: event.target.value })} placeholder="max lag ms" aria-label="Max signal lag" />
      </section>

      <section className="content-grid">
        <div className="table-pane">
          <div className="pane-title">
            <Search size={16} />
            <span>{loading ? "Loading" : `${rows.length} trades`}</span>
            <span className="last-updated">{lastUpdated ? lastUpdated.toLocaleTimeString() : ""}</span>
          </div>
          {error ? <div className="error-row">{error}</div> : null}
          <div className="trade-table-wrap">
            <table className="trade-table">
              <thead>
                <tr>
                  <th>Seen</th>
                  <th>Action</th>
                  <th>Slot Δ</th>
                  <th>Same-slot TXs</th>
                  <th>Cross-slot</th>
                  <th>Decode/build</th>
                  <th>Scan/submit</th>
                  <th>Parse/ack</th>
                  <th>Copy</th>
                  <th>Local/sig</th>
                  <th>Telegram subscriber</th>
                  <th>CA</th>
                  <th>Route</th>
                  <th>Signal/exec</th>
                  <th>Wallet</th>
                  <th>Copy wallet</th>
                  <th>Tx</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((row) => {
                  const latency = benchmarkLatencyCells(row);
                  return (
                    <tr key={row.id} className={selected?.id === row.id ? "selected" : ""} onClick={() => setSelectedId(row.id)}>
                      <td>{new Date(row.observedAtMs).toLocaleTimeString()}</td>
                      <td><span className={`status ${actionTone(row.action)}`}>{row.action}</span></td>
                      <td>{slotDelta(row.execution)}</td>
                      <td><span className={`status ${positionTone(row.execution)}`}>{sameSlotTxsAfterWallet(row.execution)}</span></td>
                      <td><span className={`status ${positionTone(row.execution)}`}>{crossSlotLanding(row.execution)}</span></td>
                      <td>{latency.decode}</td>
                      <td>{latency.scan}</td>
                      <td>{latency.txParse}</td>
                      <td><span className={`status ${decisionTone(row.execution)}`}>{copyStatus(row.execution)}</span></td>
                      <td className="strong">{latency.local}</td>
                      <td className="mono">{short(row.telegramSubscriber, 4)}</td>
                      <td className="mono">{short(row.mint, 4)}</td>
                      <td>{row.route}</td>
                      <td className="strong">{latency.block}</td>
                      <td className="mono">{short(row.targetWallet, 4)}</td>
                      <td className="mono">{short(row.copyWallet, 4)}</td>
                      <td className="mono">{short(row.signature, 4)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>

        <aside className="inspector-pane">
          {selected ? (
            <>
              <div className="pane-title inspector-title">
                <Activity size={16} />
                <span>{short(selected.mint, 8)}</span>
              </div>
              {detailError ? <div className="error-row">{detailError}</div> : null}
              <MetricGrid items={[
                { label: "Parsed - blockTime", value: ms(firstNumber(selectedSignal?.observedMinusBlockTimeMs, subtractMs(selectedExecution?.matchedAtMs, selectedExecution?.targetBlockTimeMs))) },
                { label: "Local detect", value: durationWithFallback(selectedSignal?.localDetectMs, selectedSignal?.localDetectUs, executionLocalDetectUs(selectedExecution)) },
                { label: "gRPC - blockTime", value: ms(firstNumber(selectedSignal?.grpcReceivedMinusBlockTimeMs, subtractMs(selectedExecution?.feedReceivedAtMs, selectedExecution?.targetBlockTimeMs))) },
                { label: "Batch scan", value: us(firstNumber(selectedSignal?.batchScanUs, selectedExecution?.batchScanUs)) },
                { label: "Tx parse", value: us(firstNumber(selectedSignal?.txParseUs, selectedExecution?.txParseUs)) },
                { label: "Legacy parse", value: durationWithFallback(selectedSignal?.parseMs, selectedSignal?.parseUs, selectedExecution?.routeParseUs) }
              ]} />
              {selectedExecution ? <ExecutionReport selectedExecution={selectedExecution} /> : <div className="execution-report empty-report">No copy execution report for this signal.</div>}
              <DetailList items={[
                { label: "Target", value: short(selected.targetWallet, 8) },
                { label: "Action", value: selected.action },
                { label: "Route", value: selected.route },
                { label: "Copyable", value: selected.copyable ? "yes" : "no" },
                { label: "SOL", value: selectedSignal?.solAmount === null || selectedSignal?.solAmount === undefined ? "n/a" : String(selectedSignal.solAmount) },
                { label: "Tokens", value: selectedSignal?.tokenAmount === null || selectedSignal?.tokenAmount === undefined ? "n/a" : String(selectedSignal.tokenAmount) },
                { label: "Target slot", value: String(selected.slot) },
                { label: "Target tx", value: short(selected.signature, 8) },
                { label: "Endpoint", value: selected.endpoint || "n/a" },
                { label: "Deserialize", value: duration(selectedSignal?.deserializeMs, selectedSignal?.deserializeUs) },
                { label: "Batch txs", value: selectedSignal?.batchTransactionCount === null || selectedSignal?.batchTransactionCount === undefined ? "n/a" : String(selectedSignal.batchTransactionCount) },
                { label: "Matched tx index", value: selectedSignal?.matchedTransactionIndex === null || selectedSignal?.matchedTransactionIndex === undefined ? "n/a" : String(selectedSignal.matchedTransactionIndex) },
                { label: "Account expand", value: us(selectedSignal?.accountExpandUs) },
                { label: "Wallet match", value: us(selectedSignal?.walletMatchUs) },
                { label: "Route parse", value: us(selectedSignal?.routeParseUs) },
                { label: "Provider", value: selected.provider }
              ]} />
              <div className="timeline">
                <TimerReset size={15} />
                <span>grpc {selectedSignal?.grpcMessageReceivedAtMs ? new Date(selectedSignal.grpcMessageReceivedAtMs).toLocaleTimeString() : "n/a"}</span>
                <span>decoded {selectedSignal?.entriesDeserializedAtMs ? new Date(selectedSignal.entriesDeserializedAtMs).toLocaleTimeString() : "n/a"}</span>
                <span>parsed {selectedSignal?.tradeParsedAtMs ? new Date(selectedSignal.tradeParsedAtMs).toLocaleTimeString() : new Date(selected.observedAtMs).toLocaleTimeString()}</span>
                <span>block {selectedSignal?.blockTimeMs ? new Date(selectedSignal.blockTimeMs).toLocaleTimeString() : "n/a"}</span>
              </div>
              <pre className="json-block">{JSON.stringify({
                row: selected,
                signal: selectedSignal,
                localExecution: selectedExecution,
                rawEvent: selectedSignal?.rawEvent
              }, null, 2)}</pre>
            </>
          ) : (
            <div className="empty-state">No signals in range.</div>
          )}
        </aside>
      </section>
    </main>
  );
}

function ExecutionReport({ selectedExecution }: { selectedExecution: LocalExecutionReport }) {
  return (
    <section className="execution-report" aria-label="Copy execution report">
      <div className="pane-title">
        <TimerReset size={15} />
        <span>Copy execution</span>
        <span className={`status ${decisionTone(selectedExecution)}`}>{copyStatus(selectedExecution)}</span>
      </div>
      <MetricGrid items={[
        { label: "Sig returned", value: ms(selectedExecution.observedToSignatureReturnedMs) },
        { label: "Slot delta", value: slotDelta(selectedExecution) },
        { label: "Position", value: positionSummary(selectedExecution) },
        { label: "Same-slot TXs", value: sameSlotTxsAfterWallet(selectedExecution) },
        { label: "Cross-slot", value: crossSlotLanding(selectedExecution) },
        { label: "Queue", value: us(selectedExecution.executorQueueUs) },
        { label: "Build/sign", value: `${us(selectedExecution.unsignedBuildUs)} / ${us(selectedExecution.signUs)}` }
      ]} />
      <DetailList items={[
        { label: "Copy wallet", value: short(selectedExecution.copyWallet, 8) },
        { label: "Copy tx", value: short(selectedExecution.sendSignature, 8) },
        { label: "Buy status", value: copyStatus(selectedExecution) },
        { label: "Buy chain error", value: chainError(selectedExecution.buyChainError) },
        { label: "Auto sell status", value: autoSellStatus(selectedExecution) },
        { label: "Sell chain error", value: chainError(selectedExecution.autoSellChainError) },
        { label: "Sell tx", value: short(selectedExecution.autoSellSendSignature, 8) },
        { label: "Target slot", value: selectedExecution.blockPositionDiagnostics?.targetSlot === null || selectedExecution.blockPositionDiagnostics?.targetSlot === undefined ? "n/a" : String(selectedExecution.blockPositionDiagnostics.targetSlot) },
        { label: "Copy slot", value: selectedExecution.blockPositionDiagnostics?.copySlot === null || selectedExecution.blockPositionDiagnostics?.copySlot === undefined ? "n/a" : String(selectedExecution.blockPositionDiagnostics.copySlot) },
        { label: "Target tx index", value: selectedExecution.blockPositionDiagnostics?.targetTxIndex === null || selectedExecution.blockPositionDiagnostics?.targetTxIndex === undefined ? "n/a" : String(selectedExecution.blockPositionDiagnostics.targetTxIndex) },
        { label: "Copy tx index", value: selectedExecution.blockPositionDiagnostics?.copyTxIndex === null || selectedExecution.blockPositionDiagnostics?.copyTxIndex === undefined ? "n/a" : String(selectedExecution.blockPositionDiagnostics.copyTxIndex) },
        { label: "Raw tx delta", value: selectedExecution.blockPositionDiagnostics?.txDelta === null || selectedExecution.blockPositionDiagnostics?.txDelta === undefined ? "n/a" : String(selectedExecution.blockPositionDiagnostics.txDelta) },
        { label: "Legacy TXs after wallet", value: txsAfterWallet(selectedExecution) },
        { label: "Position status", value: selectedExecution.blockPositionDiagnostics?.status || "n/a" },
        { label: "Position reason", value: selectedExecution.blockPositionDiagnostics?.unavailableReason || "n/a" },
        { label: "Route layout", value: selectedExecution.routeLayout || "n/a" },
        { label: "Observed spend", value: sol(selectedExecution.observedSolAmount) },
        { label: "Max spend", value: sol(selectedExecution.maxCopySol) },
        { label: "Network fee", value: sol(selectedExecution.networkFeeSol) },
        { label: "Token fill", value: amount(selectedExecution.fillTokenDelta) },
        { label: "Sim units", value: selectedExecution.simulationUnitsConsumed === null ? "n/a" : String(selectedExecution.simulationUnitsConsumed) },
        { label: "Local detect", value: us(executionLocalDetectUs(selectedExecution)) },
        { label: "Feed decode", value: us(selectedExecution.feedReceivedToDecodedUs) },
        { label: "Batch scan", value: us(selectedExecution.batchScanUs) },
        { label: "Tx parse", value: us(selectedExecution.txParseUs) },
        { label: "Account expand", value: us(selectedExecution.accountExpandUs) },
        { label: "Wallet match", value: us(selectedExecution.walletMatchUs) },
        { label: "Route parse", value: us(selectedExecution.routeParseUs) },
        { label: "Executor queue", value: us(selectedExecution.executorQueueUs) },
        { label: "Guards", value: us(selectedExecution.guardsUs) },
        { label: "Unsigned build", value: us(selectedExecution.unsignedBuildUs) },
        { label: "Sign", value: us(selectedExecution.signUs) },
        { label: "Serialize", value: us(selectedExecution.serializeUs) },
        { label: "Send lane", value: ms(selectedExecution.sendLaneMs) },
        { label: "Signed", value: ms(selectedExecution.observedToSignedMs) },
        { label: "Sim/send", value: ms(selectedExecution.observedToSendSubmittedMs) },
        { label: "Gross spend", value: sol(selectedExecution.grossCopySpendSol) },
        { label: "Extra spend", value: sol(selectedExecution.extraSpendBeyondObservedAndNetworkFeeSol) },
        { label: "Reason", value: selectedExecution.reason || "n/a" }
      ]} />
    </section>
  );
}
