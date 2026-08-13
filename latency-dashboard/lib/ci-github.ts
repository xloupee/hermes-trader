import type {
  CiArtifact,
  CiCheck,
  CiCheckState,
  CiDashboardPayload,
  CiJob,
  CiLogsPayload,
  CiPullRequest,
  CiRun,
  CiRunSummary,
  CiStep
} from "@/lib/ci-types";

interface GithubActor {
  login?: string;
}

interface GithubStep {
  number: number;
  name: string;
  status: string;
  conclusion: string | null;
  started_at: string | null;
  completed_at: string | null;
}

interface GithubJob {
  id: number;
  name: string;
  status: string;
  conclusion: string | null;
  started_at: string | null;
  completed_at: string | null;
  html_url: string;
  runner_name?: string | null;
  steps?: GithubStep[];
}

interface GithubArtifact {
  id: number;
  name: string;
  size_in_bytes: number;
  expired: boolean;
  created_at: string;
  expires_at: string;
  archive_download_url: string;
}

interface GithubRun {
  id: number;
  run_number: number;
  name: string;
  display_title?: string;
  status: string;
  conclusion: string | null;
  event: string;
  head_branch: string | null;
  head_sha: string;
  head_commit?: { message?: string } | null;
  actor?: GithubActor | null;
  created_at: string;
  run_started_at: string | null;
  updated_at: string;
  html_url: string;
  run_attempt?: number;
}

interface GithubRunsResponse {
  workflow_runs: GithubRun[];
}

interface GithubPullRequest {
  number: number;
  title: string;
  draft: boolean;
  html_url: string;
  head: {
    ref: string;
    sha: string;
  };
  user?: GithubActor | null;
  created_at: string;
  updated_at: string;
}

interface GithubPullRequestsResponse extends Array<GithubPullRequest> {}

interface GithubCheckRun {
  id: number;
  name: string;
  status: string;
  conclusion: string | null;
  started_at: string | null;
  completed_at: string | null;
  html_url: string | null;
  details_url?: string | null;
  app?: {
    slug?: string | null;
    name?: string | null;
  } | null;
  output?: {
    title?: string | null;
    summary?: string | null;
    text?: string | null;
  } | null;
}

interface GithubCheckRunsResponse {
  check_runs: GithubCheckRun[];
}

interface GithubCommitStatus {
  id: number;
  context: string;
  state: string;
  target_url: string | null;
  description: string | null;
  created_at: string;
  updated_at: string;
}

interface GithubCommitStatusesResponse extends Array<GithubCommitStatus> {}

interface GithubJobsResponse {
  jobs: GithubJob[];
}

interface GithubArtifactsResponse {
  artifacts: GithubArtifact[];
}

const API_ROOT = "https://api.github.com";
const DEFAULT_REPOSITORY = "xloupee/pumpfun-migration-bot";

function token(): string | undefined {
  return process.env.GITHUB_TOKEN || process.env.GH_TOKEN || process.env.GITHUB_PAT || undefined;
}

/**
 * Repositories this dashboard is allowed to query. The GitHub token is a server-side
 * credential with access to every repository its owner can read, so a caller-supplied
 * `repo` that merely looks well-formed would otherwise turn these routes into a proxy for
 * reading private pull requests, runs, and job logs anywhere that token reaches.
 */
function allowedRepositories(): string[] {
  const configured = (process.env.CI_ALLOWED_REPOSITORIES || process.env.CI_REPOSITORY || DEFAULT_REPOSITORY)
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
  return configured.length > 0 ? configured : [DEFAULT_REPOSITORY];
}

/** Marks a rejected request so routes can answer 400 without matching on message text. */
export function isCiBadRequest(error: unknown): boolean {
  return typeof error === "object" && error !== null && "status" in error &&
    Number((error as { status?: unknown }).status) === 400;
}

function badRequest(message: string): Error {
  return Object.assign(new Error(message), { status: 400 });
}

function safeRepository(value: string | null | undefined): string {
  const allowed = allowedRepositories();
  const repository = value?.trim() || process.env.CI_REPOSITORY || allowed[0];
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
    throw badRequest("Repository must use the owner/name format.");
  }
  if (!allowed.some((entry) => entry.toLowerCase() === repository.toLowerCase())) {
    throw badRequest("Repository is not allowed.");
  }
  return repository;
}

/**
 * The configured repository, for error paths that must not throw. Falls back to the first
 * allowlist entry when CI_REPOSITORY is absent from the allowlist.
 */
export function configuredRepository(): string {
  const allowed = allowedRepositories();
  const configured = process.env.CI_REPOSITORY?.trim();
  if (configured && allowed.some((entry) => entry.toLowerCase() === configured.toLowerCase())) {
    return configured;
  }
  return allowed[0];
}

function parseRunId(value: string | null): number | undefined {
  if (!value) {
    return undefined;
  }
  const runId = Number(value);
  if (!Number.isSafeInteger(runId) || runId <= 0) {
    throw badRequest("Run ID must be a positive integer.");
  }
  return runId;
}

function durationMs(startedAt: string | null, completedAt: string | null, status: string): number | null {
  if (!startedAt) {
    return null;
  }
  const start = Date.parse(startedAt);
  const end = completedAt ? Date.parse(completedAt) : Date.now();
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) {
    return null;
  }
  if (status !== "completed" && !completedAt && end === start) {
    return null;
  }
  return end - start;
}

function mapStep(step: GithubStep): CiStep {
  return {
    number: step.number,
    name: step.name,
    status: step.status,
    conclusion: step.conclusion,
    startedAt: step.started_at,
    completedAt: step.completed_at,
    durationMs: durationMs(step.started_at, step.completed_at, step.status)
  };
}
function mapJob(job: GithubJob): CiJob {
  return {
    id: job.id,
    name: job.name,
    status: job.status,
    conclusion: job.conclusion,
    startedAt: job.started_at,
    completedAt: job.completed_at,
    durationMs: durationMs(job.started_at, job.completed_at, job.status),
    url: job.html_url,
    runnerName: job.runner_name || null,
    steps: (job.steps || []).map(mapStep)
  };
}

function mapArtifact(artifact: GithubArtifact): CiArtifact {
  return {
    id: artifact.id,
    name: artifact.name,
    sizeInBytes: artifact.size_in_bytes,
    expired: artifact.expired,
    createdAt: artifact.created_at,
    expiresAt: artifact.expires_at,
    downloadUrl: artifact.archive_download_url || null
  };
}

function mapSummary(run: GithubRun): CiRunSummary {
  return {
    id: run.id,
    runNumber: run.run_number,
    name: run.name,
    displayTitle: run.display_title || run.name,
    status: run.status,
    conclusion: run.conclusion,
    event: run.event,
    branch: run.head_branch || "detached",
    sha: run.head_sha,
    actor: run.actor?.login || "unknown",
    createdAt: run.created_at,
    startedAt: run.run_started_at,
    updatedAt: run.updated_at,
    durationMs: durationMs(run.run_started_at, run.status === "completed" ? run.updated_at : null, run.status),
    url: run.html_url
  };
}

function mapRun(run: GithubRun, jobs: CiJob[], artifacts: CiArtifact[]): CiRun {
  return {
    ...mapSummary(run),
    commitMessage: run.head_commit?.message?.split("\n")[0] || run.display_title || run.name,
    runAttempt: run.run_attempt || 1,
    jobs,
    artifacts
  };
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "GitHub returned an unknown error.";
}

function mapCheckRun(check: GithubCheckRun): CiCheck {
  return {
    id: String(check.id),
    name: check.name,
    kind: "check",
    status: check.status || "queued",
    conclusion: check.conclusion,
    startedAt: check.started_at,
    completedAt: check.completed_at,
    durationMs: durationMs(check.started_at, check.completed_at, check.status),
    url: check.html_url || null,
    detailsUrl: check.details_url || null,
    app: check.app?.name || check.app?.slug || null,
    outputTitle: check.output?.title || null,
    summary: check.output?.summary || null,
    outputText: check.output?.text || null
  };
}

function mapCommitStatus(status: GithubCommitStatus): CiCheck {
  const state = status.state.toLowerCase();
  const pending = state === "pending";
  const conclusion = state === "success"
    ? "success"
    : state === "failure" || state === "error"
      ? "failure"
      : pending
        ? null
        : "neutral";
  return {
    id: `status-${status.id}`,
    name: status.context,
    kind: "status",
    status: pending ? "pending" : "completed",
    conclusion,
    startedAt: status.created_at,
    completedAt: pending ? null : status.updated_at,
    durationMs: durationMs(status.created_at, pending ? null : status.updated_at, pending ? "pending" : "completed"),
    url: status.target_url,
    detailsUrl: status.target_url,
    app: null,
    outputTitle: null,
    summary: status.description,
    outputText: null
  };
}

function summarizeChecks(checks: CiCheck[]): {
  state: CiPullRequest["checkState"];
  counts: CiPullRequest["checkCounts"];
} {
  const completed = checks.filter((check) => check.status === "completed").length;
  const success = checks.filter((check) => check.conclusion === "success").length;
  const failure = checks.filter((check) => ["failure", "timed_out", "action_required", "cancelled"].includes(check.conclusion || "")).length;
  const running = checks.filter((check) => check.status === "in_progress").length;
  const queued = checks.filter((check) => ["queued", "waiting", "requested", "pending"].includes(check.status)).length;
  const state: CiPullRequest["checkState"] = failure > 0
    ? "failure"
    : running > 0
      ? "running"
      : queued > 0
        ? "queued"
        : checks.length === 0
          ? "none"
          : success === checks.length
            ? "success"
            : "neutral";
  return {
    state,
    counts: {
      total: checks.length,
      completed,
      success,
      failure,
      running,
      queued
    }
  };
}

async function mapPullRequest(repository: string, pullRequest: GithubPullRequest): Promise<{
  pullRequest: CiPullRequest;
  warning: string | null;
}> {
  const [checkRunsResult, statusesResult] = await Promise.allSettled([
    githubFetch<GithubCheckRunsResponse>(`/repos/${repository}/commits/${pullRequest.head.sha}/check-runs?per_page=100`),
    githubFetch<GithubCommitStatusesResponse>(`/repos/${repository}/commits/${pullRequest.head.sha}/statuses?per_page=100`)
  ]);
  const checks: CiCheck[] = [];
  const warnings: string[] = [];
  if (checkRunsResult.status === "fulfilled") {
    checks.push(...checkRunsResult.value.check_runs.map(mapCheckRun));
  } else {
    warnings.push(`check runs unavailable: ${errorMessage(checkRunsResult.reason)}`);
  }
  if (statusesResult.status === "fulfilled") {
    checks.push(...statusesResult.value.map(mapCommitStatus));
  } else {
    warnings.push(`commit statuses unavailable: ${errorMessage(statusesResult.reason)}`);
  }
  const summary = summarizeChecks(checks);
  return {
    pullRequest: {
      number: pullRequest.number,
      title: pullRequest.title,
      branch: pullRequest.head.ref,
      sha: pullRequest.head.sha,
      draft: pullRequest.draft,
      author: pullRequest.user?.login || "unknown",
      createdAt: pullRequest.created_at,
      updatedAt: pullRequest.updated_at,
      url: pullRequest.html_url,
      checks,
      checkState: summary.state,
      checkCounts: summary.counts
    },
    warning: warnings.length ? `PR #${pullRequest.number}: ${warnings.join("; ")}` : null
  };
}

async function githubFetch<T>(path: string): Promise<T> {
  const response = await fetch(`${API_ROOT}${path}`, {
    cache: "no-store",
    headers: {
      Accept: "application/vnd.github+json",
      ...(token() ? { Authorization: `Bearer ${token()}` } : {}),
      "X-GitHub-Api-Version": "2022-11-28"
    }
  });

  if (!response.ok) {
    let detail = response.statusText;
    try {
      const body = await response.json() as { message?: string };
      detail = body.message || detail;
    } catch {
      // Keep the HTTP status when GitHub does not return JSON.
    }
    throw new Error(`GitHub API ${response.status}: ${detail}`);
  }

  return response.json() as Promise<T>;
}

export function repositoryFromInput(value: string | null | undefined): string {
  return safeRepository(value);
}

export function runIdFromInput(value: string | null): number | undefined {
  return parseRunId(value);
}

export async function getCiDashboard(repositoryInput?: string | null, requestedRunId?: number): Promise<CiDashboardPayload> {
  const repository = safeRepository(repositoryInput);
  const [pullRequestsResult, runsResult] = await Promise.allSettled([
    githubFetch<GithubPullRequestsResponse>(
      `/repos/${repository}/pulls?state=open&sort=updated&direction=desc&per_page=12`
    ),
    githubFetch<GithubRunsResponse>(
      `/repos/${repository}/actions/runs?per_page=10`
    )
  ]);
  if (pullRequestsResult.status === "rejected" && runsResult.status === "rejected") {
    throw new Error(`GitHub PR feed failed: ${errorMessage(pullRequestsResult.reason)} Actions history failed: ${errorMessage(runsResult.reason)}`);
  }

  const warnings: string[] = [];
  let pullRequests: CiPullRequest[] = [];
  if (pullRequestsResult.status === "fulfilled") {
    const mappedPullRequests = await Promise.all(
      pullRequestsResult.value.slice(0, 12).map((pullRequest) => mapPullRequest(repository, pullRequest))
    );
    pullRequests = mappedPullRequests.map(({ pullRequest }) => pullRequest);
    warnings.push(...mappedPullRequests.flatMap(({ warning }) => warning ? [warning] : []));
  } else {
    warnings.push(`PR feed unavailable: ${errorMessage(pullRequestsResult.reason)}`);
  }

  let summaries: CiRunSummary[] = [];
  let selectedRun: CiRun | null = null;
  if (runsResult.status === "fulfilled") {
    const list = runsResult.value;
    summaries = list.workflow_runs.map(mapSummary);
    let selectedRaw: GithubRun | undefined;
    try {
      selectedRaw = requestedRunId
        ? await githubFetch<GithubRun>(`/repos/${repository}/actions/runs/${requestedRunId}`)
        : list.workflow_runs[0];
    } catch (error) {
      warnings.push(`Selected Actions run unavailable: ${errorMessage(error)}`);
    }

    if (selectedRaw) {
      const [jobsResult, artifactsResult] = await Promise.allSettled([
        githubFetch<GithubJobsResponse>(
          `/repos/${repository}/actions/runs/${selectedRaw.id}/jobs?per_page=100`
        ),
        githubFetch<GithubArtifactsResponse>(
          `/repos/${repository}/actions/runs/${selectedRaw.id}/artifacts?per_page=100`
        )
      ]);
      if (jobsResult.status === "rejected") {
        warnings.push(`Actions jobs unavailable: ${errorMessage(jobsResult.reason)}`);
      }
      if (artifactsResult.status === "rejected") {
        warnings.push(`Actions artifacts unavailable: ${errorMessage(artifactsResult.reason)}`);
      }
      selectedRun = mapRun(
        selectedRaw,
        jobsResult.status === "fulfilled" ? jobsResult.value.jobs.map(mapJob) : [],
        artifactsResult.status === "fulfilled" ? artifactsResult.value.artifacts.map(mapArtifact) : []
      );
      summaries = summaries.some((run) => run.id === selectedRun?.id)
        ? summaries
        : [selectedRun, ...summaries].slice(0, 10);
    }
  } else {
    warnings.push(`Actions history unavailable: ${errorMessage(runsResult.reason)}`);
  }

  return {
    repository,
    source: "github",
    warning: warnings.length ? warnings.join(" ") : null,
    fetchedAt: new Date().toISOString(),
    runs: summaries,
    selectedRun,
    pullRequests
  };
}

export async function getCiLogs(repositoryInput: string | null | undefined, jobId: number): Promise<CiLogsPayload> {
  const repository = safeRepository(repositoryInput);
  const response = await fetch(`${API_ROOT}/repos/${repository}/actions/jobs/${jobId}/logs`, {
    cache: "no-store",
    headers: {
      Accept: "application/vnd.github+json",
      ...(token() ? { Authorization: `Bearer ${token()}` } : {}),
      "X-GitHub-Api-Version": "2022-11-28"
    }
  });

  if (!response.ok) {
    let detail = response.statusText;
    try {
      const body = await response.json() as { message?: string };
      detail = body.message || detail;
    } catch {
      // Keep the HTTP status when GitHub does not return JSON.
    }
    throw new Error(`GitHub logs ${response.status}: ${detail}`);
  }

  return {
    repository,
    jobId,
    source: "github",
    warning: null,
    text: await response.text()
  };
}
