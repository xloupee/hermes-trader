"use client";

import { buildFixture } from "@/lib/customer-config/fixtures";
import { createConfigDiff } from "@/lib/customer-config/diff.mjs";
import type {
  ApplyRequest,
  ApplyResult,
  FixtureScenario,
  HermesConfigClient,
  OperationResult,
  StoredFixtureState
} from "@/lib/customer-config/types";

const STORAGE_KEY = "hermes.customer-config.v1";
const SCENARIOS = new Set<FixtureScenario>(["active", "pending", "failed", "conflict", "empty", "unlinked", "missing-wallet"]);

function delay(milliseconds: number) {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}

function readStoredState(): StoredFixtureState {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return buildFixture("active");
    const parsed = JSON.parse(raw) as StoredFixtureState;
    if (!SCENARIOS.has(parsed.scenario) || !parsed.config || !Array.isArray(parsed.activity)) {
      return buildFixture("active");
    }
    return parsed;
  } catch {
    return buildFixture("active");
  }
}

function storeState(state: StoredFixtureState) {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
}

export class FixtureConfigClient implements HermesConfigClient {
  async load() {
    await delay(120);
    return readStoredState();
  }

  async selectScenario(scenario: FixtureScenario) {
    await delay(90);
    const state = buildFixture(scenario);
    storeState(state);
    return state;
  }

  async apply(request: ApplyRequest): Promise<ApplyResult> {
    await delay(720);

    if (request.scenario === "conflict") {
      return {
        operationId: "demo-conflict",
        status: "conflict",
        message: "Telegram saved a newer revision. Reload the active settings before applying this draft."
      };
    }
    if (request.scenario === "failed") {
      return {
        operationId: "demo-failed",
        status: "failed",
        message: "The demo planner rejected this revision. Your previous settings remain active."
      };
    }

    const config = structuredClone(request.config);
    config.revision = request.expectedRevision + 1;
    const stored = readStoredState();
    const changeCount = createConfigDiff(stored.config, request.config).length;
    const status = request.scenario === "pending" ? "pending" : "active";
    storeState({
      ...stored,
      config,
      activity: [
        {
          id: `demo-operation-${config.revision}`,
          type: "configuration",
          title: status === "active" ? `Revision ${config.revision} became active` : `Revision ${config.revision} is publishing`,
          detail: `${changeCount} demo ${changeCount === 1 ? "change" : "changes"} applied locally.`,
          status,
          occurredAt: "Just now"
        },
        ...stored.activity
      ]
    });

    return {
      operationId: `demo-operation-${config.revision}`,
      status,
      config
    };
  }

  async status(operationId: string): Promise<OperationResult> {
    await delay(240);
    return { operationId, status: "active" };
  }
}

export const fixtureConfigClient = new FixtureConfigClient();
