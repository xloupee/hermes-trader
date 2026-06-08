"use client";

import { useCallback, useEffect, useState } from "react";

export function useAutoRefreshQuery<T>(fetcher: () => Promise<T>, options?: { intervalMs?: number }) {
  const intervalMs = options?.intervalMs ?? 1000;
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [paused, setPaused] = useState(false);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  const refresh = useCallback(async () => {
    setError(null);
    const next = await fetcher();
    setData(next);
    setLastUpdated(new Date());
    return next;
  }, [fetcher]);

  useEffect(() => {
    let cancelled = false;
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
  }, [refresh]);

  useEffect(() => {
    if (paused) {
      return;
    }
    const timer = window.setInterval(() => {
      refresh().catch((loadError) => setError(loadError instanceof Error ? loadError.message : String(loadError)));
    }, intervalMs);
    return () => window.clearInterval(timer);
  }, [intervalMs, paused, refresh]);

  return {
    data,
    loading,
    error,
    paused,
    setPaused,
    lastUpdated,
    refresh
  };
}
