import { redirect } from "next/navigation";

export default async function SignalsPage() {
  redirect("/dashboard/executions");
}
