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
  description: string;
  startedAt: string | null;
  completedAt: string | null;
  duration: number | null;
  tone: Tone;
  evidence: string | null;
  source: "reported" | "derived" | "planned";
}

interface BuildRecord {
  key: string;
  name: string;
  sourceLabel: string;
  runner: string | null;
  startedAt: string | null;
  completedAt: string | null;
  duration: number | null;
  tone: Tone;
  check: CiCheck | null;
  job: CiJob | null;
  steps: CiStep[];
  outputLines: string[];
  hasDetailedOutput: boolean;
  hasDownloadOutput: boolean;
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
  builds: BuildRecord[];
  reportedCount: number;
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

const PHASE_DESCRIPTIONS: Record<string, string> = {
  resolve_exact_merge: "Resolve the PR head against its base and lock the exact merge identity.",
  prepare_runner: "Check disk headroom, caches, dependencies, and the required toolchain.",
  repository_integrity: "Verify the repository diff and fail on malformed or unsafe changes.",
  npm_ci: "Install the exact Node dependency lockfile.",
  portable_preflight: "Validate portable build inputs before platform-specific work begins.",
  typescript_build: "Compile the TypeScript application and catch type or bundling errors.",
  node_suite: "Run the Node-only test suite.",
  rust_format: "Verify Rust formatting without changing source files.",
  rust_check: "Type-check the full Rust workspace and all targets.",
  debug_validator: "Build and fingerprint the runtime validator used by cross-language tests.",
  cross_language_node_suite: "Run Node tests against the validated Rust binary.",
  full_node_suite: "Run the complete cross-language Node validation suite.",
  vps_ci_tests: "Exercise VPS CI policy, receipt, and promotion contracts.",
  release_build: "Build the exact release artifacts once for the selected merge.",
  release_validator: "Verify the release validator is present, executable, and sealed.",
  linux_gates: "Run Linux, systemd, socket, and kernel compatibility gates.",
  restart_regressions: "Run restart, rollback, and AX-safe integration regressions.",
  gateway_hotfix_build: "Build the scoped gateway hotfix artifact.",
  gateway_hotfix_verify: "Verify hotfix identity and runtime compatibility.",
  optional_control_check: "Validate affected control-plane code when present.",
  run_validation: "Execute the tests and platform gates required by this risk tier.",
  seal_promotion: "Seal the artifact, receipt, and promotion evidence.",
  seal_evidence: "Publish the final receipt and immutable build evidence."
};

function phaseDescription(name: string): string {
  return PHASE_DESCRIPTIONS[name] || "Run the reporter-defined validation for this phase.";
}

function checkOutputLines(check: CiCheck): string[] {
  const sections = [check.outputTitle, check.summary, check.outputText]
    .filter((value): value is string => Boolean(value?.trim()))
    .filter((value, index, values) => values.indexOf(value) === index);
  return sections
    .flatMap((value) => value?.split("\n") || [])
    .map((line) => line.trim().replace(/^```(?:text)?$|^```$/g, ""))
    .filter(Boolean);
}

const DOWNLOAD_OUTPUT = /\b(downloading|downloaded|fetching|installing|compiling|receiving objects|resolving packages|npm (?:ci|install)|cargo (?:fetch|build|check)|git fetch|curl|wget)\b/i;

function plannedPhaseNames(context: CheckContext): string[] {
  if (context.tier === "fast") {
    return ["resolve_exact_merge", "repository_integrity", "seal_evidence"];
  }
  if (context.tier === "hotfix" || context.route === "routine") {
    return ["resolve_exact_merge", "prepare_runner", "gateway_hotfix_build", "gateway_hotfix_verify", "optional_control_check", "seal_evidence"];
  }
  if (context.tier === "standard") {
    return ["resolve_exact_merge", "prepare_runner", "npm_ci", "typescript_build", "node_suite", "seal_evidence"];
  }
  return [
    "resolve_exact_merge",
    "prepare_runner",
    "npm_ci",
    "typescript_build",
    "rust_format",
    "rust_check",
    "debug_validator",
    "cross_language_node_suite",
    "vps_ci_tests",
    "seal_evidence"
  ];
}

function completeAt(startedAt: string | null, duration: number | null, fallback: string | null): string | null {
  if (startedAt && duration !== null) {
    const timestamp = Date.parse(startedAt);
    if (Number.isFinite(timestamp)) return new Date(timestamp + duration).toISOString();
  }
  return fallback;
}

function phasesFromCheck(check: CiCheck): CiPhase[] {
  const lines = checkOutputLines(check).map((line) => line.replaceAll("`", ""));
  const phases: CiPhase[] = [];
  const byName = new Map<string, CiPhase>();

  const getPhase = (name: string): CiPhase => {
    const existing = byName.get(name);
    if (existing) return existing;
    const created: CiPhase = {
      key: `${check.kind}-${check.id}-${name}`,
      name,
      description: phaseDescription(name),
      startedAt: check.startedAt,
      completedAt: null,
      duration: null,
      tone: "neutral",
      evidence: null,
      source: "reported"
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

function processPhases(check: CiCheck | null, context: CheckContext): { phases: CiPhase[]; reportedCount: number } {
  const reported = check ? phasesFromCheck(check) : [];
  const byName = new Map(reported.map((phase) => [phase.name, phase]));
  const phases = plannedPhaseNames(context).map((name): CiPhase => {
    const published = byName.get(name);
    if (published) {
      byName.delete(name);
      return published;
    }
    if (name === "resolve_exact_merge" && context.mergeSha) {
      return {
        key: `${check?.kind || "check"}-${check?.id || "unknown"}-derived-${name}`,
        name,
        description: phaseDescription(name),
        startedAt: check?.startedAt || null,
        completedAt: check?.startedAt || null,
        duration: null,
        tone: "success",
        evidence: `Exact merge ${context.mergeSha} is present in the published check context.`,
        source: "derived"
      };
    }
    return {
      key: `${check?.kind || "check"}-${check?.id || "unknown"}-planned-${name}`,
      name,
      description: phaseDescription(name),
      startedAt: null,
      completedAt: null,
      duration: null,
      tone: "neutral",
      evidence: null,
      source: "planned"
    };
  });
  phases.push(...byName.values());
  return { phases, reportedCount: reported.length };
}

function checkContext(check: CiCheck | null): CheckContext {
  const summary = check ? checkOutputLines(check).join("\n") : "";
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

function checkBuilds(pr: CiPullRequest): BuildRecord[] {
  return pr.checks.map((check) => {
    const outputLines = checkOutputLines(check);
    return {
      key: `${check.kind}-${check.id}-build`,
      name: check.name,
      sourceLabel: check.app || "GitHub check",
      runner: null,
      startedAt: check.startedAt,
      completedAt: check.completedAt,
      duration: check.durationMs,
      tone: toneFor(check.status, check.conclusion),
      check,
      job: null,
      steps: [],
      outputLines,
      hasDetailedOutput: Boolean(check.outputText?.trim()) || outputLines.length > 1,
      hasDownloadOutput: outputLines.some((line) => DOWNLOAD_OUTPUT.test(line))
    };
  });
}

function workflowBuilds(pr: CiPullRequest, run: CiRun | null): BuildRecord[] {
  if (!run || (run.sha !== pr.sha && run.branch !== pr.branch)) return [];
  return run.jobs.map((job) => ({
    key: `job-${job.id}`,
    name: job.name,
    sourceLabel: run.name,
    runner: job.runnerName,
    startedAt: job.startedAt,
    completedAt: job.completedAt,
    duration: job.durationMs,
    tone: toneFor(job.status, job.conclusion),
    check: null,
    job,
    steps: job.steps,
    outputLines: [],
    hasDetailedOutput: false,
    hasDownloadOutput: false
  }));
}

function phaseReport(pr: CiPullRequest, run: CiRun | null = null): PhaseReport {
  const check = vpsCheck(pr) || pr.checks[0] || null;
  const context = checkContext(check);
  const workflow = workflowBuilds(pr, run);
  const workflowJobIds = new Set(workflow.flatMap((build) => build.job ? [String(build.job.id)] : []));
  const workflowJobUrls = new Set(workflow.flatMap((build) => build.job ? [build.job.url] : []));
  const builds = [
    ...checkBuilds(pr).filter((build) => {
      if (!build.check) return true;
      const checkUrl = build.check.url || build.check.detailsUrl;
      return !workflowJobIds.has(String(build.check.id)) && (!checkUrl || !workflowJobUrls.has(checkUrl));
    }),
    ...workflow
  ];
  const process = processPhases(check, context);
  const phases = process.phases;
  const passed = phases.filter((phase) => phase.tone === "success").length;
  const failed = phases.filter((phase) => phase.tone === "failure").length;
  const running = phases.filter((phase) => phase.tone === "running").length;
  const current = [...phases].reverse().find((phase) => phase.source === "reported" && (phase.tone === "running" || phase.tone === "queued")) || null;

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
      ? "VPS runner active · stage stream pending"
      : check && ["queued", "waiting", "requested", "pending"].includes(check.status)
        ? "Waiting for VPS runner"
        : check?.conclusion === "success"
          ? "Build passed · detailed stage stream unavailable"
          : check
            ? "Build complete · inspect VPS output"
            : "Waiting for VPS build";
  const evidenced = phases.filter((phase) => phase.source !== "planned").length;
  const phaseSummary = `${phases.length} expected stages · ${evidenced} evidenced${process.reportedCount ? ` · ${process.reportedCount} reporter marker${process.reportedCount === 1 ? "" : "s"}` : ""}`;
  const progressLabel = phases.length ? `${passed}/${phases.length}` : "—";

  return {
    check,
    context,
    phases,
    builds,
    reportedCount: process.reportedCount,
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
    const output = checkOutputLines(check);
    return [{
      key: `${check.kind}-${check.id}`,
      title: check.name,
      summary: check.outputTitle || output[0] || check.name,
      time: check.startedAt || check.completedAt,
      duration: check.durationMs,
      tone: toneFor(check.status, check.conclusion),
      job: null,
      step: null,
      check,
      evidence: output.join("\n") || null
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
  const detailReport = selectedPr ? phaseReport(selectedPr, selectedRun) : null;
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
                <div className={styles.titleBlock}>
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

              <div className={styles.runMeta} aria-label="Build context">
                <span>{detailReport.context.mode ? `${phaseTitle(detailReport.context.mode)} mode` : "Mode unknown"}</span>
                <span>{detailReport.context.tier ? `${phaseTitle(detailReport.context.tier)} tier` : "Tier unknown"}</span>
                <span>{detailReport.context.route ? `${phaseTitle(detailReport.context.route)} route` : "Route unknown"}</span>
                <span>Risk {detailReport.context.pathRisk ? phaseTitle(detailReport.context.pathRisk) : "unknown"}</span>
                <span>Merge {detailReport.context.mergeSha?.slice(0, 12) || "pending"}</span>
              </div>

              <section className={styles.breakdown} aria-label="Detailed CI process">
                <div className={styles.breakdownHead}>
                  <div>
                    <span className={styles.eyebrow}>Process map</span>
                    <strong>{detailReport.phaseLabel}</strong>
                    <small>{detailReport.phaseSummary}</small>
                  </div>
                  <div className={styles.progressSummary}>
                    <strong>{detailReport.progressLabel}</strong>
                    <span>{formatDuration(detailReport.check?.durationMs || selectedRun?.durationMs || null)} elapsed</span>
                  </div>
                </div>
                <span className={`${styles.phaseRail} ${styles.detailRail}`} aria-label={`${detailReport.progressLabel} expected stages evidenced`}>
                  {detailReport.phases.map((phase) => <i className={toneClass(phase.tone)} key={phase.key} title={`${phaseTitle(phase.name)} · ${phase.source === "planned" ? "No VPS marker published" : phase.source === "derived" ? "Confirmed by check context" : stateLabel(phase.tone)}`} />)}
                </span>
                {detailReport.phases.length ? (
                  <div className={styles.phaseList}>
                    {detailReport.phases.map((phase, index) => (
                      <div className={styles.phaseRow} key={phase.key}>
                        <span className={`${styles.phaseMarker} ${toneClass(phase.tone)}`}><StatusMark tone={phase.tone} /></span>
                        <span className={styles.phaseIndex}>{String(index + 1).padStart(2, "0")}</span>
                        <span className={styles.phaseName}><strong>{phaseTitle(phase.name)}</strong><small>{phase.description}</small></span>
                        <span className={styles.phaseTiming}>
                          <time>{phase.source === "planned" ? "Not published" : formatTime(phase.startedAt)}</time>
                          <small>{phase.source === "planned" ? "Expected · awaiting VPS output" : phase.source === "derived" ? "Confirmed by check context" : `${stateLabel(phase.tone)} · ${formatDuration(phase.duration)}`}</small>
                        </span>
                      </div>
                    ))}
                  </div>
                ) : (
                  <div className={styles.telemetryEmpty}>
                    <CircleDashed size={17} />
                    <div><strong>{detailReport.phaseLabel}</strong><small>{detailReport.phaseSummary}. Refreshing this record every 12 seconds.</small></div>
                  </div>
                )}
                <p className={styles.breakdownNote}>
                  {detailReport.context.tier || detailReport.context.route
                    ? "This is the build path selected for the PR."
                    : "The VPS reporter has not published a route yet, so Hermes is showing the full critical build path."}
                  {" "}Stages remain visible, but Hermes only marks a stage when the VPS check publishes evidence for it.
                </p>
              </section>

              <section className={styles.buildFeed} aria-label="Actual VPS builds">
                <div className={styles.buildFeedHead}>
                  <div>
                    <span className={styles.eyebrow}>Actual VPS builds</span>
                    <strong>{detailReport.builds.length} published build record{detailReport.builds.length === 1 ? "" : "s"}</strong>
                    <small>Direct GitHub check or Actions records. Nothing in this section is inferred from the process map.</small>
                  </div>
                  <span className={styles.feedRefresh}><i />refreshes every 12s</span>
                </div>

                {detailReport.builds.length ? detailReport.builds.map((build) => (
                  <article className={styles.buildRecord} key={build.key}>
                    <div className={styles.buildRecordHead}>
                      <span className={`${styles.buildPulse} ${toneClass(build.tone)}`}><StatusMark tone={build.tone} /></span>
                      <div className={styles.buildIdentity}>
                        <strong>{build.name}</strong>
                        <small>{build.sourceLabel}{build.runner ? ` · ${build.runner}` : ""}</small>
                      </div>
                      <Status tone={build.tone} />
                    </div>

                    <div className={styles.buildFacts}>
                      <span><small>Started</small><strong>{formatTime(build.startedAt)}</strong></span>
                      <span><small>Elapsed</small><strong>{formatDuration(build.duration)}</strong></span>
                      <span><small>Completed</small><strong>{formatTime(build.completedAt)}</strong></span>
                      <span><small>Output</small><strong>{build.hasDetailedOutput ? "Detailed stream" : build.outputLines.length ? "Status context only" : "Not published"}</strong></span>
                    </div>

                    {build.steps.length > 0 && (
                      <div className={styles.buildSteps} aria-label={`${build.name} steps`}>
                        {build.steps.map((step) => {
                          const tone = toneFor(step.status, step.conclusion);
                          return <span key={`${build.key}-${step.number}`}><i className={toneClass(tone)} /><b>{String(step.number).padStart(2, "0")}</b><strong>{step.name}</strong><small>{stateLabel(tone)} · {formatDuration(step.durationMs)}</small></span>;
                        })}
                      </div>
                    )}

                    <div className={styles.vpsTerminal}>
                      <div className={styles.terminalHead}><span><SquareTerminal size={13} />Published VPS output</span><small>{build.hasDownloadOutput ? "download / build activity detected" : "live via GitHub"}</small></div>
                      {build.outputLines.length ? (
                        <ol>{build.outputLines.map((line, index) => <li className={DOWNLOAD_OUTPUT.test(line) ? styles.downloadLine : undefined} key={`${build.key}-output-${index}`}><b>{String(index + 1).padStart(2, "0")}</b><code>{line}</code></li>)}</ol>
                      ) : (
                        <div className={styles.terminalEmpty}>No output lines have been published for this build.</div>
                      )}
                      {build.check && !build.hasDetailedOutput && <p className={styles.streamNote}>The VPS reporter currently publishes only a status headline for this build. Hermes will print commands, downloads, compiler output, and diagnostics here automatically when they appear in the GitHub check output.</p>}
                      {build.hasDetailedOutput && !build.hasDownloadOutput && <p className={styles.streamNote}>Detailed VPS output is available, but no package-download or build-command lines are present in the latest published record.</p>}
                    </div>

                    <div className={styles.buildActions}>
                      {build.job && <button type="button" onClick={() => void loadLog(build.job!)} disabled={loadingLog}>{loadingLog && logJobId === build.job.id ? "Loading full log…" : "Load full GitHub job log"}<SquareTerminal size={13} /></button>}
                      {build.job && <a href={build.job.url} target="_blank" rel="noreferrer">Open GitHub job <ExternalLink size={12} /></a>}
                      {build.check && (build.check.url || build.check.detailsUrl) && <a href={build.check.url || build.check.detailsUrl || "#"} target="_blank" rel="noreferrer">Open VPS check <ExternalLink size={12} /></a>}
                    </div>
                    {build.job && logText && build.job.id === logJobId && <pre className={styles.log}>{logText.slice(-12_000)}</pre>}
                  </article>
                )) : <div className={styles.telemetryEmpty}><CircleDashed size={17} /><div><strong>No build record published</strong><small>Hermes is still polling GitHub for this PR head.</small></div></div>}
              </section>

              <details className={styles.evidenceDisclosure}>
                <summary><span>Raw evidence</span><small>{events.length} GitHub record{events.length === 1 ? "" : "s"}</small><ChevronDown size={14} /></summary>
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
                  }) : <div className={styles.empty}><CircleDashed size={17} />No raw workflow or phase evidence has been published for this head.</div>}
                </div>
              </details>
            </>
          )}
        </section>
      ) : (
        <section className={styles.ledger}>
          <div className={styles.ledgerMeta}>
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
