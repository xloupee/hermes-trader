export type UserTimeValue = number | string | Date;

export function resolveUserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

function dateValue(value: UserTimeValue): Date {
  return value instanceof Date ? value : new Date(value);
}

export function formatUserTime(
  value: UserTimeValue,
  timeZone: string,
  locale?: string | string[]
): string {
  return dateValue(value).toLocaleTimeString(locale, {
    hour: "numeric",
    minute: "2-digit",
    second: "2-digit",
    hour12: true,
    timeZone
  });
}

export function formatUserDate(
  value: UserTimeValue,
  timeZone: string,
  locale?: string | string[]
): string {
  return dateValue(value).toLocaleDateString(locale, {
    month: "short",
    day: "numeric",
    timeZone
  });
}

export function formatUserDateTime(
  value: UserTimeValue,
  timeZone: string,
  locale?: string | string[]
): string {
  return dateValue(value).toLocaleString(locale, {
    dateStyle: "medium",
    timeStyle: "medium",
    hour12: true,
    timeZone
  });
}

export function userTimeZoneLabel(
  timeZone: string,
  value: UserTimeValue = new Date(),
  locale?: string | string[]
): string {
  const formatter = new Intl.DateTimeFormat(locale, { timeZone, timeZoneName: "short" });
  return formatter.formatToParts(dateValue(value)).find((part) => part.type === "timeZoneName")?.value || timeZone;
}
