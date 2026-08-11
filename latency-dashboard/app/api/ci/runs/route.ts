import {
  getCiDashboard,
  isCiBadRequest,
  repositoryFromInput,
  runIdFromInput
} from "@/lib/ci-github";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(request: Request) {
  const searchParams = new URL(request.url).searchParams;

  try {
    const repository = repositoryFromInput(searchParams.get("repo"));
    const runId = runIdFromInput(searchParams.get("run"));
    return Response.json(await getCiDashboard(repository, runId), {
      headers: { "Cache-Control": "no-store" }
    });
  } catch (error) {
    if (isCiBadRequest(error)) {
      const message = error instanceof Error ? error.message : "Invalid request.";
      return Response.json({ error: message }, { status: 400 });
    }
    const detail = error instanceof Error ? error.message : "Live GitHub CI feed unavailable.";
    return Response.json({ error: detail }, {
      status: 503,
      headers: { "Cache-Control": "no-store" }
    });
  }
}
