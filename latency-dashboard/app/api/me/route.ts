import { authErrorResponse, requireAdmin } from "@/lib/auth";

export async function GET() {
  try {
    const { user, email } = await requireAdmin();
    return Response.json({
      user: {
        id: user.id,
        email
      },
      isAdmin: true
    });
  } catch (error) {
    return authErrorResponse(error);
  }
}
