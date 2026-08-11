import type { ConfigDiff, CustomerConfig } from "@/lib/customer-config/types";

export function createConfigDiff(before: CustomerConfig, after: CustomerConfig): ConfigDiff[];
export function plannedExposure(config: CustomerConfig): number;
