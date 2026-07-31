"use client";

import { useCallback, useMemo } from "react";
import type { Route } from "next";
import { usePathname, useRouter, useSearchParams } from "next/navigation";
import {
  DEFAULT_FILTERS,
  type DashboardFilterState,
  type LandingPreset,
  parseDashboardFilters,
  toQueryParams
} from "@/lib/dashboard-client";

export interface UseDashboardFiltersState {
  filters: DashboardFilterState;
  setFilters: (filters: Partial<DashboardFilterState>) => void;
  setOutcome: (outcome: LandingPreset) => void;
  resetFilters: () => void;
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
    router.replace(dashboardRoute(pathname, toQueryParams({ ...filters, ...next }, true)));
  }, [filters, pathname, router]);

  const setOutcome = useCallback((outcome: LandingPreset) => {
    const clearPresetSide = outcome === "all" && filters.outcome !== "all";
    const next = { ...filters, side: clearPresetSide ? "" : filters.side, outcome };
    router.replace(dashboardRoute(pathname, toQueryParams(next, true)));
  }, [filters, pathname, router]);

  const resetFilters = useCallback(() => {
    router.replace(dashboardRoute(pathname, toQueryParams(DEFAULT_FILTERS, true)));
  }, [pathname, router]);

  return { filters, setFilters, setOutcome, resetFilters };
}
