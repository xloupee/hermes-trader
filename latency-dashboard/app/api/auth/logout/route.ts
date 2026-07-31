import { NextResponse } from "next/server";
import { OPERATOR_SESSION_COOKIE } from "@/lib/operator-session.mjs";
import { createClient } from "@/lib/supabase/server";

export async function POST() {
  try {
    const supabase = await createClient();
    await supabase.auth.signOut();
  } catch {
    // The operator cookie must still be revoked if Supabase is unavailable.
  }
  const response = NextResponse.json({ ok: true });
  response.cookies.set(OPERATOR_SESSION_COOKIE, "", { httpOnly: true, path: "/", maxAge: 0 });
  response.headers.set("Cache-Control", "private, no-store");
  return response;
}
