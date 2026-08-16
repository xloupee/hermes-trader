import type { DashboardExecution } from "@/lib/dashboard-contract.mjs";
import type { GatewayConfirmation } from "@/lib/gateway-confirmations";

const RPC_TIMEOUT_MS = 3000;
const CLUSTER_NODES_TTL_MS = 5 * 60 * 1000;
const GEO_TTL_MS = 60 * 60 * 1000;
const PUBLICNODE_SOLANA_RPC_URL = "https://solana-rpc.publicnode.com";

interface ClusterNode {
  pubkey: string;
  gossip?: string | null;
  version?: string | null;
}

interface GeoInfo {
  query: string;
  status: string;
  countryCode?: string;
  regionName?: string;
  city?: string;
  as?: string;
}

interface EpochSchedule {
  firstNormalSlot: number;
  slotsPerEpoch: number;
}

export interface LeaderInfo {
  identity: string | null;
  shortIdentity: string | null;
  ip: string | null;
  location: string | null;
  broadRegion: string | null;
  network: string | null;
}

export interface LeaderDiagnostics {
  targetSlot: number | null;
  copySlot: number | null;
  targetLeader: LeaderInfo | null;
  copyLeader: LeaderInfo | null;
  leaderChanged: boolean | null;
  regionPath: string | null;
}

let clusterNodesCache: { expiresAt: number; promise: Promise<Map<string, ClusterNode>> } | null = null;
let epochScheduleCache: Promise<EpochSchedule | null> | null = null;
const slotLeaderCache = new Map<number, Promise<string | null>>();
const leaderScheduleCache = new Map<number, Promise<Map<number, string>>>();
const geoCache = new Map<string, { expiresAt: number; value: GeoInfo | null }>();

function rpcUrls(): string[] {
  return [...new Set([
    process.env.JITO_SYNC_RPC_URL,
    process.env.JITO_BLOCK_POSITION_RPC_URL,
    process.env.SOLANA_RPC_URL,
    process.env.NEXT_PUBLIC_SOLANA_RPC_URL,
    PUBLICNODE_SOLANA_RPC_URL
  ].filter((url): url is string => Boolean(url)))];
}

async function rpc<T>(method: string, params: unknown[]): Promise<T | null> {
  for (const url of rpcUrls()) {
    const abort = new AbortController();
    const timeout = setTimeout(() => abort.abort(), RPC_TIMEOUT_MS);
    try {
      const response = await fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
        signal: abort.signal
      });
      const body = await response.json();
      if (response.ok && !body?.error && body?.result !== undefined) return body.result;
    } catch {
      // Try the next read-only RPC.
    } finally {
      clearTimeout(timeout);
    }
  }
  return null;
}

function shortIdentity(value: string | null): string | null {
  return value && value.length > 14 ? `${value.slice(0, 6)}...${value.slice(-6)}` : value;
}

function ipFromGossip(value: string | null | undefined): string | null {
  const ip = value?.split(":")[0]?.trim();
  return ip || null;
}

function broadRegion(countryCode: string | null | undefined): string | null {
  if (!countryCode) return null;
  if (["DE", "NL", "GB", "LT", "SE", "IE", "FR", "FI", "PL"].includes(countryCode)) return "Europe";
  if (["US", "CA"].includes(countryCode)) return "North America";
  if (["JP", "SG", "KR", "HK"].includes(countryCode)) return "Asia";
  return countryCode;
}

function locationLabel(geo: GeoInfo | null): string | null {
  if (!geo || geo.status !== "success") return null;
  const place = geo.city || geo.regionName;
  return [place, geo.countryCode].filter(Boolean).join(", ") || null;
}

async function clusterNodes(): Promise<Map<string, ClusterNode>> {
  const now = Date.now();
  if (!clusterNodesCache || clusterNodesCache.expiresAt <= now) {
    clusterNodesCache = {
      expiresAt: now + CLUSTER_NODES_TTL_MS,
      promise: rpc<ClusterNode[]>("getClusterNodes", []).then((nodes) =>
        new Map((nodes || []).map((node) => [node.pubkey, node]))
      )
    };
  }
  return clusterNodesCache.promise;
}

async function epochSchedule(): Promise<EpochSchedule | null> {
  if (!epochScheduleCache) epochScheduleCache = rpc<EpochSchedule>("getEpochSchedule", []);
  return epochScheduleCache;
}

async function slotLeader(slot: number): Promise<string | null> {
  if (!slotLeaderCache.has(slot)) {
    slotLeaderCache.set(slot, scheduledLeader(slot).then(async (scheduled) =>
      scheduled ?? (await rpc<string[]>("getSlotLeaders", [slot, 1]))?.[0] ?? null
    ));
  }
  return slotLeaderCache.get(slot) ?? null;
}

async function scheduledLeader(slot: number): Promise<string | null> {
  const schedule = await epochSchedule();
  if (!schedule || schedule.slotsPerEpoch <= 0 || slot < schedule.firstNormalSlot) return null;
  const epochStart = schedule.firstNormalSlot
    + Math.floor((slot - schedule.firstNormalSlot) / schedule.slotsPerEpoch) * schedule.slotsPerEpoch;
  const relativeSlot = slot - epochStart;
  if (!leaderScheduleCache.has(epochStart)) {
    leaderScheduleCache.set(epochStart, rpc<Record<string, number[]>>("getLeaderSchedule", [slot]).then((value) => {
      const leaders = new Map<number, string>();
      for (const [identity, relativeSlots] of Object.entries(value || {})) {
        for (const relative of relativeSlots) leaders.set(relative, identity);
      }
      return leaders;
    }));
  }
  return (await leaderScheduleCache.get(epochStart))?.get(relativeSlot) ?? null;
}

async function geoForIps(ips: string[]): Promise<Map<string, GeoInfo | null>> {
  const now = Date.now();
  const uniqueIps = [...new Set(ips)].filter(Boolean);
  const missing = uniqueIps.filter((ip) => !geoCache.has(ip) || (geoCache.get(ip)?.expiresAt ?? 0) <= now);
  if (missing.length > 0) {
    const abort = new AbortController();
    const timeout = setTimeout(() => abort.abort(), RPC_TIMEOUT_MS);
    try {
      const response = await fetch("http://ip-api.com/batch?fields=status,countryCode,regionName,city,query,as", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(missing),
        signal: abort.signal
      });
      const body = await response.json();
      const rows = Array.isArray(body) ? body as GeoInfo[] : [];
      for (const ip of missing) {
        geoCache.set(ip, { expiresAt: now + GEO_TTL_MS, value: rows.find((row) => row.query === ip) ?? null });
      }
    } catch {
      for (const ip of missing) geoCache.set(ip, { expiresAt: now + 60_000, value: null });
    } finally {
      clearTimeout(timeout);
    }
  }
  return new Map(uniqueIps.map((ip) => [ip, geoCache.get(ip)?.value ?? null]));
}

function candidateSlots(row: DashboardExecution): { targetSlot: number | null; copySlot: number | null } {
  const targetSlot = row.targetSlot ?? row.blockPositionDiagnostics?.targetSlot ?? row.slot ?? null;
  const copySlot = row.copySlot
    ?? row.blockPositionDiagnostics?.copySlot
    ?? (typeof targetSlot === "number" && typeof row.slotDelta === "number" ? targetSlot + row.slotDelta : null);
  return {
    targetSlot: typeof targetSlot === "number" && Number.isFinite(targetSlot) ? targetSlot : null,
    copySlot: typeof copySlot === "number" && Number.isFinite(copySlot) ? copySlot : null
  };
}

function leaderInfo(identity: string | null, node: ClusterNode | undefined, geo: GeoInfo | null): LeaderInfo | null {
  if (!identity) return null;
  return {
    identity,
    shortIdentity: shortIdentity(identity),
    ip: ipFromGossip(node?.gossip),
    location: locationLabel(geo),
    broadRegion: broadRegion(geo?.countryCode),
    network: geo?.as || node?.version || null
  };
}

function regionPath(target: LeaderInfo | null, copy: LeaderInfo | null): string | null {
  const targetRegion = target?.location || target?.broadRegion;
  const copyRegion = copy?.location || copy?.broadRegion;
  if (!targetRegion && !copyRegion) return null;
  if (!targetRegion || targetRegion === copyRegion) return copyRegion ?? null;
  if (!copyRegion) return targetRegion;
  return `${targetRegion} -> ${copyRegion}`;
}

export async function enrichGatewayConfirmationsWithLeaderDiagnostics(
  rows: GatewayConfirmation[]
): Promise<GatewayConfirmation[]> {
  if (rows.length === 0) return rows;

  const nodes = await clusterNodes();
  const identities = [...new Set(rows.flatMap((row) => [row.targetSlotLeader, row.copySlotLeader])
    .filter((identity): identity is string => Boolean(identity)))];
  const ips = identities.map((identity) => ipFromGossip(nodes.get(identity)?.gossip))
    .filter((ip): ip is string => Boolean(ip));
  const geoByIp = await geoForIps(ips);

  return rows.map((row) => {
    const targetNode = row.targetSlotLeader ? nodes.get(row.targetSlotLeader) : undefined;
    const copyNode = row.copySlotLeader ? nodes.get(row.copySlotLeader) : undefined;
    const targetIp = ipFromGossip(targetNode?.gossip);
    const copyIp = ipFromGossip(copyNode?.gossip);
    const targetLeader = leaderInfo(
      row.targetSlotLeader,
      targetNode,
      targetIp ? geoByIp.get(targetIp) ?? null : null
    );
    const copyLeader = leaderInfo(
      row.copySlotLeader,
      copyNode,
      copyIp ? geoByIp.get(copyIp) ?? null : null
    );
    return {
      ...row,
      leaderDiagnostics: {
        targetSlot: row.slot,
        copySlot: row.confirmationSlot,
        targetLeader,
        copyLeader,
        leaderChanged: row.targetSlotLeader && row.copySlotLeader
          ? row.targetSlotLeader !== row.copySlotLeader
          : null,
        regionPath: regionPath(targetLeader, copyLeader)
      }
    };
  });
}

export async function enrichExecutionsWithLeaderDiagnostics(rows: DashboardExecution[]): Promise<DashboardExecution[]> {
  const pairs = rows.map(candidateSlots);
  const slots = [...new Set(pairs.flatMap((pair) => [pair.targetSlot, pair.copySlot])
    .filter((slot): slot is number => typeof slot === "number"))];
  if (slots.length === 0) return rows.map((row) => ({ ...row, leaderDiagnostics: null }));

  const [nodes, leaders] = await Promise.all([
    clusterNodes(),
    Promise.all(slots.map(async (slot) => [slot, await slotLeader(slot)] as const))
  ]);
  const leaderBySlot = new Map(leaders);
  const ips = leaders.map(([, identity]) => ipFromGossip(identity ? nodes.get(identity)?.gossip : null))
    .filter((ip): ip is string => Boolean(ip));
  const geoByIp = await geoForIps(ips);

  return rows.map((row, index) => {
    const { targetSlot, copySlot } = pairs[index];
    const targetIdentity = targetSlot === null ? null : leaderBySlot.get(targetSlot) ?? null;
    const copyIdentity = copySlot === null ? null : leaderBySlot.get(copySlot) ?? null;
    const targetNode = targetIdentity ? nodes.get(targetIdentity) : undefined;
    const copyNode = copyIdentity ? nodes.get(copyIdentity) : undefined;
    const targetIp = ipFromGossip(targetNode?.gossip);
    const copyIp = ipFromGossip(copyNode?.gossip);
    const targetLeader = leaderInfo(targetIdentity, targetNode, targetIp ? geoByIp.get(targetIp) ?? null : null);
    const copyLeader = leaderInfo(copyIdentity, copyNode, copyIp ? geoByIp.get(copyIp) ?? null : null);
    return {
      ...row,
      leaderDiagnostics: {
        targetSlot,
        copySlot,
        targetLeader,
        copyLeader,
        leaderChanged: targetIdentity && copyIdentity ? targetIdentity !== copyIdentity : null,
        regionPath: regionPath(targetLeader, copyLeader)
      }
    };
  });
}
