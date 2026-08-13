import {
  getCiLogs,
  isCiBadRequest,
  repositoryFromInput
} from "@/lib/ci-github";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

export async function GET(request: Request) {
  const searchParams = new URL(request.url).searchParams;
  const jobId = Number(searchParams.get("jobId"));

  try {
    const repository = repositoryFromInput(searchParams.get("repo"));
    if (!Number.isSafeInteger(jobId) || jobId <= 0) {
      return Response.json({ error: "Job ID must be a positive integer." }, { status: 400 });
    }
    return Response.json(await getCiLogs(repository, jobId), {
      headers: { "Cache-Control": "no-store" }
    });
  } catch (error) {
    if (isCiBadRequest(error)) {
      const message = error instanceof Error ? error.message : "Invalid request.";
      return Response.json({ error: message }, { status: 400 });
    }
    const detail = error instanceof Error ? error.message : "Live GitHub logs unavailable.";
    return Response.json({ error: detail }, {
      status: 503,
      headers: { "Cache-Control": "no-store" }
    });
  }
}
