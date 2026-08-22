export type CiStatus =
  | "queued"
  | "in_progress"
  | "completed"
  | "waiting"
  | "requested"
  | "pending"
  | string;

export type CiConclusion =
  | "success"
  | "failure"
  | "cancelled"
  | "skipped"
  | "neutral"
  | "timed_out"
  | "action_required"
  | null
  | string;

export interface CiStep {
  number: number;
  name: string;
  status: CiStatus;
  conclusion: CiConclusion;
  startedAt: string | null;
  completedAt: string | null;
  durationMs: number | null;
}

export interface CiJob {
  id: number;
  name: string;
  status: CiStatus;
  conclusion: CiConclusion;
  startedAt: string | null;
  completedAt: string | null;
  durationMs: number | null;
  url: string;
  runnerName: string | null;
  steps: CiStep[];
}

export interface CiArtifact {
  id: number;
  name: string;
  sizeInBytes: number;
  expired: boolean;
  createdAt: string | null;
  expiresAt: string | null;
  downloadUrl: string | null;
}

export type CiCheckKind = "check" | "status";
export type CiCheckState = "success" | "failure" | "running" | "queued" | "none" | "neutral";

export interface CiCheck {
  id: string;
  name: string;
  kind: CiCheckKind;
  status: CiStatus;
  conclusion: CiConclusion;
  startedAt: string | null;
  completedAt: string | null;
  durationMs: number | null;
  url: string | null;
  detailsUrl: string | null;
  app: string | null;
  outputTitle: string | null;
  summary: string | null;
  outputText: string | null;
}

export interface CiPullRequest {
  number: number;
  title: string;
  branch: string;
  sha: string;
  draft: boolean;
  author: string;
  createdAt: string;
  updatedAt: string;
  url: string;
  checks: CiCheck[];
  checkState: CiCheckState;
  checkCounts: {
    total: number;
    completed: number;
    success: number;
    failure: number;
    running: number;
    queued: number;
  };
}

export interface CiRunSummary {
  id: number;
  runNumber: number;
  name: string;
  displayTitle: string;
  status: CiStatus;
  conclusion: CiConclusion;
  event: string;
  branch: string;
  sha: string;
  actor: string;
  createdAt: string;
  startedAt: string | null;
  updatedAt: string;
  durationMs: number | null;
  url: string;
}

export interface CiRun extends CiRunSummary {
  commitMessage: string;
  runAttempt: number;
  jobs: CiJob[];
  artifacts: CiArtifact[];
}

export interface CiDashboardPayload {
  repository: string;
  source: "github";
  warning: string | null;
  fetchedAt: string;
  runs: CiRunSummary[];
  selectedRun: CiRun | null;
  pullRequests: CiPullRequest[];
}

export interface CiLogsPayload {
  repository: string;
  jobId: number;
  source: "github";
  warning: string | null;
  text: string;
}
