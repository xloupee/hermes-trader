"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import {
  ArrowLeft,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  CircleDashed,
  ExternalLink,
  LoaderCircle,
  RefreshCw,
  ServerCog,
  SquareTerminal,
  XCircle
} from "lucide-react";
import type { CiCheck, CiConclusion, CiDashboardPayload, CiJob, CiPullRequest, CiRun, CiStep } from "@/lib/ci-types";
import { DashboardNav } from "@/components/dashboard/dashboard-nav";
import styles from "@/app/ci/ci-ledger.module.css";

// The server resolves which repository to query from its own configuration and rejects
// anything outside the allowlist, so the client deliberately does not send one.

type Tone = "success" | "failure" | "running" | "queued" | "neutral";

function toneFor(status: string, conclusion: CiConclusion): Tone {
  if (status === "in_progress") return "running";
  if (["queued", "waiting", "requested", "pending"].includes(status)) return "queued";
  if (conclusion === "success") return "success";
  if (["failure", "timed_out", "action_required"].includes(conclusion || "")) return "failure";
  return "neutral";
}

function toneClass(tone: Tone): string {
  return {
    success: styles.success,
    failure: styles.failure,
    running: styles.running,
    queued: styles.queued,
    neutral: styles.neutral
  }[tone];
}

function stateLabel(tone: Tone): string {
  return { success: "Passed", failure: "Failed", running: "Running", queued: "Queued", neutral: "No result" }[tone];
}

function formatDuration(value: number | null): string {
  if (value === null || !Number.isFinite(value)) return "—";
  const seconds = Math.max(0, Math.round(value / 1000));
  const minutes = Math.floor(seconds / 60);
  return minutes ? `${minutes}m ${String(seconds % 60).padStart(2, "0")}s` : `${seconds}s`;
}

function formatTime(value: string | null): string {
  if (!value) return "—";
  return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit", second: "2-digit" }).format(new Date(value));
}

function relativeTime(value: string): string {
  const seconds = Math.max(0, Math.round((Date.now() - new Date(value).getTime()) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.floor(minutes / 60)}h ago`;
}

function StatusMark({ tone }: { tone: Tone }) {
  if (tone === "success") return <CheckCircle2 size={14} aria-hidden="true" />;
  if (tone === "failure") return <XCircle size={14} aria-hidden="true" />;
  if (tone === "running") return <LoaderCircle className={styles.spin} size={14} aria-hidden="true" />;
  return <CircleDashed size={14} aria-hidden="true" />;
}

function Status({ tone }: { tone: Tone }) {
  return <span className={`${styles.status} ${toneClass(tone)}`}><StatusMark tone={tone} />{stateLabel(tone)}</span>;
}

interface DetailEvent {
  key: string;
  title: string;
  summary: string;
  time: string | null;
  duration: number | null;
  tone: Tone;
  job: CiJob | null;
  step: CiStep | null;
  check: CiCheck | null;
  evidence: string | null;
}

interface CiPhase {
  key: string;
  name: string;
  startedAt: string | null;
  completedAt: string | null;
  duration: number | null;
  tone: Tone;
  evidence: string | null;
}

interface CheckContext {
  mergeSha: string | null;
  mode: string | null;
  tier: string | null;
  route: string | null;
  pathRisk: string | null;
}

interface PhaseReport {
  check: CiCheck | null;
  context: CheckContext;
  phases: CiPhase[];
  current: CiPhase | null;
  tone: Tone;
  phaseLabel: string;
  phaseSummary: string;
  progressLabel: string;
  passed: number;
  failed: number;
  running: number;
}

function eventTime(event: DetailEvent): number {
  return event.time ? new Date(event.time).getTime() : 0;
}

function phaseTitle(value: string): string {
  return value.replaceAll("_", " ").replace(/\b\w/g, (character) => character.toUpperCase());
}

function completeAt(startedAt: string | null, duration: number | null, fallback: string | null): string | null {
  if (startedAt && duration !== null) {
    const timestamp = Date.parse(startedAt);
    if (Number.isFinite(timestamp)) return new Date(timestamp + duration).toISOString();
  }
  return fallback;
}

function phasesFromCheck(check: CiCheck): CiPhase[] {
  const lines = check.summary?.split("\n").map((line) => line.trim().replaceAll("`", "")).filter(Boolean) || [];
  const phases: CiPhase[] = [];
  const byName = new Map<string, CiPhase>();

  const getPhase = (name: string): CiPhase => {
    const existing = byName.get(name);
    if (existing) return existing;
    const created: CiPhase = {
      key: `${check.kind}-${check.id}-${name}`,
      name,
      startedAt: check.startedAt,
      completedAt: null,
      duration: null,
      tone: "neutral",
      evidence: null
    };
    byName.set(name, created);
    phases.push(created);
    return created;
  };

  for (const line of lines) {
    const start = line.match(/PHASE_START\s+name=([^\s]+)\s+at=([^\s]+)/);
    if (start) {
      const phase = getPhase(start[1]);
      phase.startedAt = start[2];
      phase.tone = check.status === "completed" ? "neutral" : "running";
      phase.evidence = line;
      continue;
    }
    const result = line.match(/PHASE_(OK|FAIL)\s+name=([^\s]+)(?:\s+seconds=(\d+))?/);
    if (result) {
      const phase = getPhase(result[2]);
      phase.duration = result[3] ? Number(result[3]) * 1000 : phase.duration;
      phase.completedAt = completeAt(phase.startedAt, phase.duration, check.completedAt);
      phase.tone = result[1] === "OK" ? "success" : "failure";
      phase.evidence = line;
    }
  }

  return phases;
}

function checkContext(check: CiCheck | null): CheckContext {
  const summary = check?.summary || "";
  return {
    mergeSha: summary.match(/(?:Exact|Testing exact) merge\s+([0-9a-f]{7,40})/i)?.[1] || null,
    mode: summary.match(/\bin\s+([a-z-]+)\s+mode\b/i)?.[1] || null,
    tier: summary.match(/\bat\s+([a-z-]+)\s+tier\b/i)?.[1] || null,
    route: summary.match(/\bvia\s+([a-z-]+)\s+route\b/i)?.[1] || null,
    pathRisk: summary.match(/Path risk is\s+([a-z-]+)/i)?.[1] || null
  };
}

function vpsCheck(pr: CiPullRequest): CiCheck | null {
  return pr.checks.find((check) => check.name.toLowerCase() === "vps pr ci" || check.app?.toLowerCase().includes("vps-ci")) || null;
}

function phaseReport(pr: CiPullRequest): PhaseReport {
  const check = vpsCheck(pr) || pr.checks[0] || null;
  const phases = check ? phasesFromCheck(check) : [];
  const passed = phases.filter((phase) => phase.tone === "success").length;
  const failed = phases.filter((phase) => phase.tone === "failure").length;
  const running = phases.filter((phase) => phase.tone === "running").length;
  const current = [...phases].reverse().find((phase) => phase.tone === "running" || phase.tone === "queued") || phases[phases.length - 1] || null;

  let tone: Tone = "neutral";
  if (failed || check?.conclusion === "failure" || check?.conclusion === "timed_out" || check?.conclusion === "action_required") {
    tone = "failure";
  } else if (running || check?.status === "in_progress") {
    tone = "running";
  } else if (check && ["queued", "waiting", "requested", "pending"].includes(check.status)) {
    tone = "queued";
  } else if (check?.conclusion === "success") {
    tone = "success";
  }

  const phaseLabel = current
    ? phaseTitle(current.name)
    : check?.status === "in_progress"
      ? "VPS runner active · phase telemetry pending"
      : check && ["queued", "waiting", "requested", "pending"].includes(check.status)
        ? "Waiting for VPS runner"
        : check
          ? "Check complete · phase list unavailable"
          : "Waiting for VPS reporter";
  const phaseSummary = phases.length
    ? `${phases.length} reported · ${passed} passed${failed ? ` · ${failed} failed` : ""}${running ? ` · ${running} active` : ""}`
    : check?.status === "in_progress"
      ? "The reporter has not published phase markers yet"
      : "No phase telemetry published";
  const progressLabel = phases.length ? `${passed}/${phases.length}` : "—";

  return {
    check,
    context: checkContext(check),
    phases,
    current,
    tone,
    phaseLabel,
    phaseSummary,
    progressLabel,
    passed,
    failed,
    running
  };
}

function eventsFromCheck(check: CiCheck): DetailEvent[] {
  const phases = phasesFromCheck(check);

  if (!phases.length) {
    return [{
      key: `${check.kind}-${check.id}`,
      title: check.name,
      summary: check.summary?.split("\n").map((line) => line.trim()).filter(Boolean)[0] || check.name,
      time: check.startedAt || check.completedAt,
      duration: check.durationMs,
      tone: toneFor(check.status, check.conclusion),
      job: null,
      step: null,
      check,
      evidence: check.summary
    }];
  }

  return phases.map((phase) => ({
    key: phase.key,
    title: phaseTitle(phase.name),
    summary: `${phase.tone === "success" ? "Phase passed" : phase.tone === "failure" ? "Phase failed" : phase.tone === "running" ? "Phase running" : "Phase unresolved"} · ${check.name}`,
    time: phase.startedAt,
    duration: phase.duration,
    tone: phase.tone,
    job: null,
    step: null,
    check,
    evidence: phase.evidence
  }));
}

function eventsFor(pr: CiPullRequest, run: CiRun | null): DetailEvent[] {
  const workflowEvents = run?.jobs.flatMap((job) => job.steps.map((step) => ({
    key: `${job.id}-${step.number}`,
    title: step.name,
    summary: job.name,
    time: step.startedAt || step.completedAt,
    duration: step.durationMs,
    tone: toneFor(step.status, step.conclusion),
    job,
    step,
    check: null,
    evidence: null
  }))) || [];
  return [...workflowEvents, ...pr.checks.flatMap(eventsFromCheck)].sort((a, b) => eventTime(b) - eventTime(a));
}

export function CILedgerDashboard() {
  const [data, setData] = useState<CiDashboardPayload | null>(null);
  const [selectedPr, setSelectedPr] = useState<CiPullRequest | null>(null);
  const [selectedRun, setSelectedRun] = useState<CiRun | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [logText, setLogText] = useState<string | null>(null);
  const [logJobId, setLogJobId] = useState<number | null>(null);
  const [loadingLog, setLoadingLog] = useState(false);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (runId?: number, quiet = false) => {
    quiet ? setRefreshing(true) : setLoading(true);
    setError(null);
    try {
      const params = new URLSearchParams();
      if (runId) params.set("run", String(runId));
      const response = await fetch(`/api/ci/runs?${params}`, { cache: "no-store" });
      const payload = await response.json() as CiDashboardPayload | { error?: string };
      if (!response.ok || !("pullRequests" in payload)) throw new Error(("error" in payload && payload.error) || "CI feed unavailable");
      setData(payload);
      if (runId) setSelectedRun(payload.selectedRun);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "CI feed unavailable");
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  useEffect(() => {
    if (!data) return undefined;
    const timer = window.setInterval(() => void load(selectedRun?.id, true), 12_000);
    return () => window.clearInterval(timer);
  }, [data, load, selectedRun?.id]);

  useEffect(() => {
    if (!data || !selectedPr) return;
    const refreshedPr = data.pullRequests.find((pr) => pr.number === selectedPr.number);
    if (refreshedPr && refreshedPr !== selectedPr) setSelectedPr(refreshedPr);
  }, [data, selectedPr]);

  const openPr = async (pr: CiPullRequest) => {
    setSelectedPr(pr);
    setSelectedRun(null);
    setLogText(null);
    const matchingRun = data?.runs.find((run) => run.branch === pr.branch || run.sha === pr.sha);
    if (matchingRun) await load(matchingRun.id);
    setExpanded(null);
  };

  const closeDetail = () => {
    setSelectedPr(null);
    setSelectedRun(null);
    setExpanded(null);
    setLogText(null);
  };

  const loadLog = async (job: CiJob) => {
    setLoadingLog(true);
    setLogJobId(job.id);
    try {
      const params = new URLSearchParams({ jobId: String(job.id) });
      const response = await fetch(`/api/ci/logs?${params}`, { cache: "no-store" });
      const payload = await response.json() as { text?: string; error?: string };
      if (!response.ok || !payload.text) throw new Error(payload.error || "Log unavailable");
      setLogText(payload.text);
    } catch (cause) {
      setLogText(cause instanceof Error ? cause.message : "Log unavailable");
    } finally {
      setLoadingLog(false);
    }
  };

  const events = useMemo(() => selectedPr ? eventsFor(selectedPr, selectedRun) : [], [selectedPr, selectedRun]);
  const detailReport = selectedPr ? phaseReport(selectedPr) : null;
  const runningCount = data?.pullRequests.filter((pr) => phaseReport(pr).tone === "running").length || 0;

  return (
    <main className={styles.shell}>
      {!selectedPr && (
        <header className={styles.header}>
          <div className={styles.brand}><span>H</span><div><b>HERMES / CI</b><small>VPS build intelligence</small></div></div>
          <div className={styles.headerCenter}>
            <DashboardNav />
          </div>
          <div className={styles.headerRight}>
            <span className={styles.live}><i />{data?.source === "github" ? "Live" : "Unavailable"}</span>
            <button type="button" onClick={() => void load(selectedRun?.id, true)} aria-label="Refresh CI data"><RefreshCw className={refreshing ? styles.spin : ""} size={14} /></button>
          </div>
        </header>
      )}

      {error && <div className={styles.error} role="alert"><XCircle size={15} />{error}<button type="button" onClick={() => void load()}>Retry</button></div>}

      {loading && !data ? (
        <div className={styles.loading}><LoaderCircle className={styles.spin} size={18} /><span><b>Reading VPS build state</b><small>Open PRs and their published evidence</small></span></div>
      ) : selectedPr ? (
        <section className={`${styles.detail} ${styles.timelineOnly}`}>
          <button className={styles.back} type="button" onClick={closeDetail}><ArrowLeft size={13} />All PR builds</button>
          {detailReport && (
            <>
              <div className={styles.detailLead}>
                <div>
                  <span className={styles.eyebrow}>Exact phase record</span>
                  <h1>#{selectedPr.number} {selectedPr.title}</h1>
                  <div className={styles.identity}>
                    <span>{selectedPr.branch}</span>
                    <span>head {selectedPr.sha.slice(0, 12)}</span>
                    {detailReport.check && <span>{detailReport.check.name}</span>}
                    <a href={selectedPr.url} target="_blank" rel="noreferrer">Open pull request <ExternalLink size={12} /></a>
                  </div>
                </div>
                <Status tone={detailReport.tone} />
              </div>

              <div className={styles.contextStrip}>
                <span><small>Mode</small><strong>{detailReport.context.mode ? `${phaseTitle(detailReport.context.mode)} mode` : "—"}</strong></span>
                <span><small>Tier</small><strong>{detailReport.context.tier ? phaseTitle(detailReport.context.tier) : "—"}</strong></span>
                <span><small>Route</small><strong>{detailReport.context.route ? phaseTitle(detailReport.context.route) : "—"}</strong></span>
                <span><small>Path risk</small><strong>{detailReport.context.pathRisk ? phaseTitle(detailReport.context.pathRisk) : "—"}</strong></span>
                <span><small>Exact merge</small><strong>{detailReport.context.mergeSha?.slice(0, 12) || "—"}</strong></span>
              </div>

              <section className={styles.breakdown} aria-label="CI phase breakdown">
                <div className={styles.breakdownHead}>
                  <div><span className={styles.eyebrow}>Phase breakdown</span><strong>{detailReport.phaseSummary}</strong></div>
                  <span>{detailReport.check ? `Reporter: ${detailReport.check.name}` : "No VPS PR CI check"}</span>
                </div>
                <div className={styles.phaseStats}>
                  <span><b>{detailReport.phases.length}</b><small>reported</small></span>
                  <span><b>{detailReport.passed}</b><small>passed</small></span>
                  <span><b>{detailReport.failed}</b><small>failed</small></span>
                  <span><b>{formatDuration(detailReport.check?.durationMs || selectedRun?.durationMs || null)}</b><small>wall time</small></span>
                </div>
                {detailReport.phases.length ? (
                  <div className={styles.phaseList}>
                    {detailReport.phases.map((phase, index) => (
                      <div className={styles.phaseRow} key={phase.key}>
                        <span className={`${styles.phaseMarker} ${toneClass(phase.tone)}`}><StatusMark tone={phase.tone} /></span>
                        <span className={styles.phaseIndex}>{String(index + 1).padStart(2, "0")}</span>
                        <span className={styles.phaseName}><strong>{phaseTitle(phase.name)}</strong><small>{stateLabel(phase.tone)}{phase.evidence ? ` · ${phase.evidence}` : ""}</small></span>
                        <span className={styles.phaseTiming}><time>{formatTime(phase.startedAt)}</time><small>{formatDuration(phase.duration)}</small></span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className={styles.telemetryEmpty}>
                    <CircleDashed size={17} />
                    <div><strong>{detailReport.phaseLabel}</strong><small>{detailReport.phaseSummary}. Refreshing this record every 12 seconds.</small></div>
                  </div>
                )}
                <p className={styles.breakdownNote}>This board counts only phase markers published by the VPS check. A green GitHub check without those markers is not treated as a detailed phase record.</p>
              </section>

              <div className={styles.evidenceHeading}><span>Published evidence</span><span>{events.length} records · GitHub checks and workflow steps</span></div>
            </>
          )}
          <div className={styles.timeline}>
            {events.length ? events.map((event) => {
              const isOpen = expanded === event.key;
              return (
                <div className={styles.event} key={event.key}>
                  <button className={styles.eventTrigger} type="button" aria-expanded={isOpen} onClick={() => { setExpanded(isOpen ? null : event.key); setLogText(null); }}>
                    <time>{formatTime(event.time)}</time>
                    <span className={`${styles.eventMark} ${toneClass(event.tone)}`}><StatusMark tone={event.tone} /></span>
                    <span className={styles.eventName}><strong>{event.title}</strong><small>{event.summary}</small></span>
                    <span className={styles.eventDuration}>{formatDuration(event.duration)}<ChevronDown size={14} /></span>
                  </button>
                  <div className={`${styles.disclosure} ${isOpen ? styles.disclosureOpen : ""}`} aria-hidden={!isOpen}>
                    <div>
                      <div className={styles.factGrid}>
                        <span><small>Status</small><strong>{stateLabel(event.tone)}</strong></span>
                        <span><small>Started</small><strong>{formatTime(event.step?.startedAt || event.time || event.check?.startedAt || null)}</strong></span>
                        <span><small>Completed</small><strong>{formatTime(event.step?.completedAt || (event.time && event.duration !== null ? new Date(new Date(event.time).getTime() + event.duration).toISOString() : event.check?.completedAt || null))}</strong></span>
                        <span><small>Duration</small><strong>{formatDuration(event.duration)}</strong></span>
                        <span className={styles.wideFact}><small>Evidence</small><strong>{event.evidence || event.check?.summary || `${event.job?.name || "VPS check"} · ${event.step?.conclusion || event.step?.status || "recorded"}`}</strong></span>
                      </div>
                      <div className={styles.eventActions}>
                        {event.job && <button type="button" onClick={() => void loadLog(event.job!)} disabled={loadingLog}>{loadingLog && logJobId === event.job.id ? "Loading log…" : "Load job log"}<SquareTerminal size={13} /></button>}
                        {(event.check?.detailsUrl || event.check?.url) && <a href={event.check.detailsUrl || event.check.url || "#"} target="_blank" rel="noreferrer">Source evidence <ExternalLink size={12} /></a>}
                      </div>
                      {logText && event.job?.id === logJobId && <pre className={styles.log}>{logText.slice(-12_000)}</pre>}
                    </div>
                  </div>
                </div>
              );
            }) : <div className={styles.empty}><CircleDashed size={17} />This PR is visible, but no workflow or phase evidence has been published for its head yet.</div>}
          </div>
        </section>
      ) : (
        <section className={styles.ledger}>
          <div className={styles.ledgerLead}>
            <div><span className={styles.eyebrow}>Exact state ledger</span><h1>VPS builds</h1><p>Open pull requests, broken into the phases the VPS reporter actually published.</p></div>
            <div className={styles.counts}><span><i />{runningCount} running</span><span>{data?.pullRequests.length || 0} in view</span></div>
          </div>
          <div className={styles.table} role="table" aria-label="VPS pull request builds">
            <div className={styles.tableHead} role="row"><span>PR / branch</span><span>Result</span><span>Current phase</span><span>Phase evidence</span><span>Updated</span></div>
            {data?.pullRequests.map((pr) => {
              const report = phaseReport(pr);
              return (
                <button className={styles.row} role="row" type="button" key={pr.number} onClick={() => void openPr(pr)} aria-label={`Open PR build #${pr.number}: ${pr.title}. ${report.phaseSummary}`}>
                  <span className={styles.prIdentity}><b>#{pr.number} {pr.title}</b><small>{pr.branch}</small></span>
                  <Status tone={report.tone} />
                  <span className={styles.phase}><strong>{report.phaseLabel}</strong><small>{report.phaseSummary}</small></span>
                  <span className={styles.progress}>
                    <span className={styles.phaseRail} aria-label={`${report.progressLabel} phases passed`}>
                      {report.phases.length ? report.phases.map((phase) => <i className={toneClass(phase.tone)} key={phase.key} title={`${phaseTitle(phase.name)} · ${stateLabel(phase.tone)}`} />) : <i className={styles.neutral} title="No phase telemetry" />}
                    </span>
                    <small>{report.progressLabel}</small>
                  </span>
                  <span className={styles.updated}>{relativeTime(pr.updatedAt)}<ChevronRight size={13} /></span>
                </button>
              );
            })}
            {!data?.pullRequests.length && <div className={styles.empty}><CircleDashed size={17} />No open pull requests are currently visible.</div>}
          </div>
          <footer className={styles.ledgerFooter}><span><ServerCog size={13} />Server-side read · credentials stay off the browser</span><span>Updated {data ? relativeTime(data.fetchedAt) : "—"}</span></footer>
        </section>
      )}
    </main>
  );
}
