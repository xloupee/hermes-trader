"use client";

import { useSyncExternalStore } from "react";
import { resolveUserTimeZone } from "@/lib/user-time";

const subscribe = () => () => undefined;

export function useUserTimeZone(): string {
  return useSyncExternalStore(subscribe, resolveUserTimeZone, () => "UTC");
}
