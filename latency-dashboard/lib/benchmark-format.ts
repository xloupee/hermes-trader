export function ms(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "n/a";
  }
  if (value > 0 && value < 1) {
    return `${Math.max(1, Math.round(value * 1000))}us`;
  }
  return `${value}ms`;
}

export function us(value: number | null | undefined): string {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    return "n/a";
  }
  if (value > 0 && value < 1000) {
    return `${Math.max(1, Math.round(value))}us`;
  }
  const msValue = value / 1000;
  if (msValue < 10) {
    return `${Math.round(msValue * 10) / 10}ms`;
  }
  return `${Math.round(msValue)}ms`;
}

export function sol(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value) ? `${value.toFixed(6)} SOL` : "n/a";
}

export function amount(value: number | null | undefined): string {
  return typeof value === "number" && Number.isFinite(value)
    ? value.toLocaleString(undefined, { maximumFractionDigits: 6 })
    : "n/a";
}

export function short(value: string | null | undefined, size = 5): string {
  if (!value) {
    return "n/a";
  }
  if (value.length <= size * 2 + 3) {
    return value;
  }
  return `${value.slice(0, size)}...${value.slice(-size)}`;
}

export function duration(msValue: number | null | undefined, usValue: number | null | undefined): string {
  if (typeof usValue === "number" && Number.isFinite(usValue)) {
    return us(usValue);
  }
  return ms(msValue);
}

export function msToUs(value: number | null | undefined): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value * 1000 : null;
}

export function queryString(filters: Record<string, string>) {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(filters)) {
    if (value.trim()) {
      params.set(key, value.trim());
    }
  }
  return params.toString();
}
