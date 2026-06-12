import { authErrorResponse, requireAdmin } from "@/lib/auth";
import { listBenchmarkRows } from "@/lib/benchmark-rows";
import { parseSignalFilters } from "@/lib/signals";

const CACHE_TTL_MS = 8000;

interface CacheEntry {
  expiresAt: number;
  response: unknown;
}

const responseCache = new Map<string, CacheEntry>();

function pruneExpiredCache(now: number): void {
  if (responseCache.size < 100) {
    return;
  }

  for (const [key, entry] of responseCache) {
    if (entry.expiresAt <= now) {
      responseCache.delete(key);
    }
  }
}

export async function GET(request: Request) {
  try {
    await requireAdmin();
    const url = new URL(request.url);
    const cacheKey = url.searchParams.toString();
    const now = Date.now();
    pruneExpiredCache(now);
    const cached = responseCache.get(cacheKey);

    if (cached && cached.expiresAt > now) {
      return Response.json(cached.response, {
        headers: {
          "Cache-Control": "private, max-age=0, must-revalidate",
          "X-Dashboard-Cache": "hit"
        }
      });
    }

    const filters = parseSignalFilters(url.searchParams);
    const response = await listBenchmarkRows(filters);
    responseCache.set(cacheKey, { expiresAt: now + CACHE_TTL_MS, response });

    return Response.json(response, {
      headers: {
        "Cache-Control": "private, max-age=0, must-revalidate",
        "X-Dashboard-Cache": "miss"
      }
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
