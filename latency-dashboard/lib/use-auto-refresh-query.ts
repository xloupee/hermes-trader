"use client";

import { useCallback, useEffect, useRef, useState } from "react";

interface AutoRefreshOptions {
  intervalMs?: number;
  pauseWhenHidden?: boolean;
  refreshOnVisible?: boolean;
  minVisibleRefreshIntervalMs?: number;
}

function pageIsHidden(): boolean {
  return typeof document !== "undefined" && document.hidden;
}

export function useAutoRefreshQuery<T>(fetcher: () => Promise<T>, options?: AutoRefreshOptions) {
  const intervalMs = options?.intervalMs ?? 5000;
  const pauseWhenHidden = options?.pauseWhenHidden ?? true;
  const refreshOnVisible = options?.refreshOnVisible ?? true;
  const minVisibleRefreshIntervalMs = options?.minVisibleRefreshIntervalMs ?? Math.min(intervalMs, 5000);
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [hidden, setHidden] = useState(pageIsHidden);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);
  const lastRefreshAtMs = useRef(0);
  const effectivePaused = paused || (pauseWhenHidden && hidden);

  const refresh = useCallback(async () => {
    setError(null);
    const next = await fetcher();
    setData(next);
    lastRefreshAtMs.current = Date.now();
    setLastUpdated(new Date());
    return next;
  }, [fetcher]);

  useEffect(() => {
    let cancelled = false;
    if (pauseWhenHidden && pageIsHidden()) {
      setLoading(false);
      return () => {
        cancelled = true;
      };
    }
    setLoading(true);
    refresh()
      .catch((loadError) => {
        if (!cancelled) {
          setError(loadError instanceof Error ? loadError.message : String(loadError));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [pauseWhenHidden, refresh]);

  useEffect(() => {
    if (!pauseWhenHidden || typeof document === "undefined") {
      return;
    }

    const handleVisibilityChange = () => {
      const nextHidden = document.hidden;
      setHidden(nextHidden);
      if (
        nextHidden ||
        paused ||
        !refreshOnVisible ||
        Date.now() - lastRefreshAtMs.current < minVisibleRefreshIntervalMs
      ) {
        return;
      }
      refresh().catch((loadError) => setError(loadError instanceof Error ? loadError.message : String(loadError)));
    };

    document.addEventListener("visibilitychange", handleVisibilityChange);
    return () => document.removeEventListener("visibilitychange", handleVisibilityChange);
  }, [minVisibleRefreshIntervalMs, pauseWhenHidden, paused, refresh, refreshOnVisible]);

  useEffect(() => {
    if (effectivePaused) {
      return;
    }
    const timer = window.setInterval(() => {
      refresh().catch((loadError) => setError(loadError instanceof Error ? loadError.message : String(loadError)));
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [effectivePaused, intervalMs, refresh]);

  return {
    data,
    loading,
    error,
    paused,
    autoPaused: !paused && pauseWhenHidden && hidden,
    setPaused,
    lastUpdated,
    refresh
  };
}
