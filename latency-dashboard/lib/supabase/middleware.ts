import { NextResponse, type NextRequest } from "next/server";
import { createServerClient, type CookieOptions } from "@supabase/ssr";
import { protectedRequestKind } from "@/lib/auth-redirect.mjs";

interface RefreshedCookie { name: string; value: string; options: CookieOptions }

function unauthenticatedResponse(request: NextRequest, kind: ReturnType<typeof protectedRequestKind>, cookies: RefreshedCookie[]) {
  const response = kind === "dashboard_page"
    ? NextResponse.redirect(new URL(`/login?next=${encodeURIComponent(request.nextUrl.pathname + request.nextUrl.search)}`, request.url))
    : NextResponse.json({ error: "unauthenticated" }, { status: 401 });
  cookies.forEach(({ name, value, options }) => response.cookies.set(name, value, options));
  response.headers.set("Cache-Control", "private, no-store");
  return response;
}

export async function updateSession(request: NextRequest) {
  let response = NextResponse.next({ request });
  let refreshedCookies: RefreshedCookie[] = [];
  const protectedKind = protectedRequestKind(request.nextUrl.pathname);
  const url = process.env.NEXT_PUBLIC_SUPABASE_URL;
  const key = process.env.NEXT_PUBLIC_SUPABASE_ANON_KEY;

  if (!url || !key) {
    return protectedKind === "none" ? response : unauthenticatedResponse(request, protectedKind, []);
  }

  const supabase = createServerClient(
    url,
    key,
    {
      cookies: {
        getAll() {
          return request.cookies.getAll();
        },
        setAll(cookiesToSet) {
          refreshedCookies = cookiesToSet;
          cookiesToSet.forEach(({ name, value }) => request.cookies.set(name, value));
          response = NextResponse.next({ request });
          cookiesToSet.forEach(({ name, value, options }) => response.cookies.set(name, value, options));
          response.headers.set("Cache-Control", "private, no-store");
        }
      }
    }
  );

  const { data } = await supabase.auth.getUser();
  if (!data.user && protectedKind !== "none") {
    return unauthenticatedResponse(request, protectedKind, refreshedCookies);
  }
  return response;
}
