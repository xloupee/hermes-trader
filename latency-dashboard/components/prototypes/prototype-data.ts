export type PrototypeSlug =
  | "ledger"
  | "briefing"
  | "tape"
  | "index"
  | "concierge"
  | "timeline"
  | "command"
  | "atelier";

export type PrototypePreset = "all" | "landed-buys" | "landed-sells" | "issues";

export interface PrototypeDirection {
  slug: PrototypeSlug;
  number: string;
  name: string;
  strapline: string;
  density: string;
  bestFor: string;
}

export interface PrototypeExecution {
  id: string;
  time: string;
  day: string;
  side: "buy" | "sell";
  outcome: "landed" | "failed" | "skipped";
  token: string;
  mint: string;
  wallet: string;
  walletShort: string;
  provider: string;
  route: string;
  landing: string;
  landingDetail: string;
  signature: string;
  signatureShort: string;
  latency: string;
}

export const PROTOTYPE_DIRECTIONS: PrototypeDirection[] = [
  {
    slug: "ledger",
    number: "01",
    name: "The Ledger",
    strapline: "A quiet, editorial operating record with the table at its center.",
    density: "Balanced",
    bestFor: "Daily scanning"
  },
  {
    slug: "briefing",
    number: "02",
    name: "Morning Briefing",
    strapline: "An executive summary that answers what happened before showing every row.",
    density: "Low",
    bestFor: "Fast health checks"
  },
  {
    slug: "tape",
    number: "03",
    name: "Execution Tape",
    strapline: "A restrained obsidian tape for operators who live in the event stream.",
    density: "High",
    bestFor: "Live monitoring"
  },
  {
    slug: "index",
    number: "04",
    name: "The Index",
    strapline: "Swiss restraint, oversized wayfinding, and almost no visual furniture.",
    density: "Balanced",
    bestFor: "Clarity and calm"
  },
  {
    slug: "concierge",
    number: "05",
    name: "Operator Concierge",
    strapline: "Priorities first: surface what needs attention, then reveal the activity.",
    density: "Low",
    bestFor: "Exception handling"
  },
  {
    slug: "timeline",
    number: "06",
    name: "Chronicle",
    strapline: "A chronological narrative where landing context reads naturally.",
    density: "Balanced",
    bestFor: "Understanding sequence"
  },
  {
    slug: "command",
    number: "07",
    name: "Command Table",
    strapline: "A filter-forward, compact workspace built around rapid investigation.",
    density: "High",
    bestFor: "Power filtering"
  },
  {
    slug: "atelier",
    number: "08",
    name: "Hermes Atelier",
    strapline: "The closest visual relative of the landing page—editorial and asymmetrical.",
    density: "Low",
    bestFor: "Brand continuity"
  }
];

export const PROTOTYPE_EXECUTIONS: PrototypeExecution[] = [
  {
    id: "execution-7804",
    time: "11:38:21",
    day: "Today",
    side: "sell",
    outcome: "landed",
    token: "ORBIT",
    mint: "2JgfRz9xeuQ7QJXfM7m3GvLxmCq3P8xpump",
    wallet: "7NAd4ZQvuRYWm5JfW2gD1kE6LxK8qCu1",
    walletShort: "7NAd…qCu1",
    provider: "Vortex",
    route: "flashx-pump",
    landing: "Landed · +1 slot",
    landingDetail: "copy slot 1355697770",
    signature: "3CVpWcA1ky6VywGrxF5B9mN2E7ZQ51fY",
    signatureShort: "3CVp…51fY",
    latency: "318 ms"
  },
  {
    id: "execution-7803",
    time: "11:34:08",
    day: "Today",
    side: "buy",
    outcome: "landed",
    token: "PALM",
    mint: "J1TUp6AtG2s9NfdwPqN88ZGrRz4Wpump",
    wallet: "PMJA8wRkJ2vN7gL4fE9T6xYQ12gyYN",
    walletShort: "PMJA…gyYN",
    provider: "Jito",
    route: "flashx-pump",
    landing: "Landed · same slot",
    landingDetail: "42 tx after target",
    signature: "4VYNG9nR3jL1QpT8kH2bZ6xWcDoUeQ",
    signatureShort: "4VYN…oUeQ",
    latency: "241 ms"
  },
  {
    id: "execution-7802",
    time: "11:27:44",
    day: "Today",
    side: "sell",
    outcome: "landed",
    token: "MOSS",
    mint: "DTiL9bK2xY7wV5mR8nC4E6qZpump",
    wallet: "B9kJ4pR8wQ2xZ6fN1mT7YvK3NcyC",
    walletShort: "B9kJ…NcyC",
    provider: "Vortex",
    route: "flashx-pump",
    landing: "Landed · slot 1355697712",
    landingDetail: "no target comparison",
    signature: "4Hrk8pL2cT7wN1vQ9zY6sMfUxb",
    signatureShort: "4Hrk…fUxb",
    latency: "289 ms"
  },
  {
    id: "execution-7801",
    time: "11:19:12",
    day: "Today",
    side: "buy",
    outcome: "failed",
    token: "TIDE",
    mint: "rxtt3mH9qW2vB7nK6sP1LyZpump",
    wallet: "8wW3tK4vJ6qL9pR1mN7xZ2LDgv",
    walletShort: "8wW3…LDgv",
    provider: "Jito",
    route: "direct-tpu",
    landing: "Failed on-chain",
    landingDetail: "custom program error 0x1",
    signature: "3ZQB6vR1yM8nK2pL9tW4qASzj5",
    signatureShort: "3ZQB…Szj5",
    latency: "404 ms"
  },
  {
    id: "execution-7800",
    time: "11:08:39",
    day: "Today",
    side: "sell",
    outcome: "landed",
    token: "COVE",
    mint: "8vL2qW9mX4pR6nT1zK7Fdpump",
    wallet: "Egmx5bN2kR8vQ1wY6tL9cZe5RU",
    walletShort: "Egmx…e5RU",
    provider: "Helius",
    route: "sender",
    landing: "Landed · +2 slots",
    landingDetail: "copy slot 1355697638",
    signature: "2enj7xK4mQ9vR1pT6bN3wZV5jT",
    signatureShort: "2enj…V5jT",
    latency: "511 ms"
  },
  {
    id: "execution-7799",
    time: "10:56:03",
    day: "Today",
    side: "buy",
    outcome: "skipped",
    token: "LILT",
    mint: "6pQ3wX8nR1tV7mK4yZ2Hapump",
    wallet: "4Dvn2mT8xQ1pL7rK5wN9cS3yZa",
    walletShort: "4Dvn…yZa",
    provider: "—",
    route: "policy",
    landing: "Skipped",
    landingDetail: "developer creation disabled",
    signature: "5SZg8kM1pR4vN9tQ2xL7wYsPxf",
    signatureShort: "5SZg…sPxf",
    latency: "—"
  }
];

export function directionForSlug(slug: string): PrototypeDirection | undefined {
  return PROTOTYPE_DIRECTIONS.find((direction) => direction.slug === slug);
}

export function filterPrototypeExecutions(preset: PrototypePreset) {
  if (preset === "landed-buys") {
    return PROTOTYPE_EXECUTIONS.filter((row) => row.side === "buy" && row.outcome === "landed");
  }
  if (preset === "landed-sells") {
    return PROTOTYPE_EXECUTIONS.filter((row) => row.side === "sell" && row.outcome === "landed");
  }
  if (preset === "issues") {
    return PROTOTYPE_EXECUTIONS.filter((row) => row.outcome !== "landed");
  }
  return PROTOTYPE_EXECUTIONS;
}
