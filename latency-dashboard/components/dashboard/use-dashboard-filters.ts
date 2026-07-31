"use client";

import { useCallback, useMemo } from "react";
import type { Route } from "next";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { DEFAULT_FILTERS, type DashboardFilterState, parseDashboardFilters, type LandingPreset } from "@/lib/dashboard-client";

export interface UseDashboardFiltersState {
  filters: DashboardFilterState;
  setFilters: (filters: Partial<DashboardFilterState>) => void;
  setOutcome: (outcome: LandingPreset) => void;
  resetFilters: () => void;
}

function sanitizeOutcome(value: string): LandingPreset {
  if (value === "landed-buys" || value === "landed-sells" || value === "non-landed") {
    return value;
  }
  return "all";
}

function dashboardRoute(pathname: string, query: string): Route {
  return (query ? `${pathname}?${query}` : pathname) as Route;
}

export function useDashboardFilters(): UseDashboardFiltersState {
  const pathname = usePathname();
  const searchParams = useSearchParams();
  const router = useRouter();

  const filters = useMemo(() => parseDashboardFilters(searchParams), [searchParams]);

  const setFilters = useCallback((next: Partial<DashboardFilterState>) => {
    const nextFilters = { ...filters, ...next };
    const params = new URLSearchParams();
    if (nextFilters.since.trim()) {
      params.set("since", nextFilters.since.trim());
    }
    if (nextFilters.provider.trim()) {
      params.set("provider", nextFilters.provider.trim());
    }
    if (nextFilters.observedWallet.trim()) {
      params.set("observedWallet", nextFilters.observedWallet.trim());
    }
    if (nextFilters.mint.trim()) {
      params.set("mint", nextFilters.mint.trim());
    }
    if (nextFilters.action.trim()) {
      params.set("action", nextFilters.action.trim());
    }
    if (nextFilters.route.trim()) {
      params.set("route", nextFilters.route.trim());
    }
    if (nextFilters.source.trim()) {
      params.set("source", nextFilters.source.trim());
    }
    if (nextFilters.outcome && nextFilters.outcome !== "all") {
      params.set("outcome", nextFilters.outcome);
    }
    const query = params.toString();
    router.replace(dashboardRoute(pathname, query));
  }, [filters, pathname, router]);

  const setOutcome = useCallback((outcome: LandingPreset) => {
    const params = new URLSearchParams(searchParams);
    if (outcome === "all") {
      params.delete("outcome");
    } else {
      params.set("outcome", outcome);
    }
    const safeOutcome = sanitizeOutcome(outcome);
    const next = { ...filters, outcome: safeOutcome };
    const filtered = new URLSearchParams();
    if (next.since.trim()) {
      filtered.set("since", next.since.trim());
    }
    if (next.provider.trim()) {
      filtered.set("provider", next.provider.trim());
    }
    if (next.observedWallet.trim()) {
      filtered.set("observedWallet", next.observedWallet.trim());
    }
    if (next.mint.trim()) {
      filtered.set("mint", next.mint.trim());
    }
    if (next.action.trim()) {
      filtered.set("action", next.action.trim());
    }
    if (next.route.trim()) {
      filtered.set("route", next.route.trim());
    }
    if (next.source.trim()) {
      filtered.set("source", next.source.trim());
    }
    if (safeOutcome !== "all") {
      filtered.set("outcome", safeOutcome);
    }
    const nextQuery = filtered.toString();
    router.replace(dashboardRoute(pathname, nextQuery));
  }, [filters, pathname, router, searchParams]);

  const resetFilters = useCallback(() => {
    const params = new URLSearchParams();
    if (DEFAULT_FILTERS.since) {
      params.set("since", DEFAULT_FILTERS.since);
    }
    const query = params.toString();
    router.replace(dashboardRoute(pathname, query));
  }, [pathname, router]);

  return { filters, setFilters, setOutcome, resetFilters };
}
