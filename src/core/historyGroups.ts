/**
 * History list helpers: group calls under "Today", "Yesterday" or their date, and
 * set aside accidental starts (under ten seconds) so they don't crowd the list.
 */
export interface DayGroup<T> {
  key: string;
  label: string;
  items: T[];
}

const SHORT_MS = 10_000;

function dayKey(ms: number): string {
  const d = new Date(ms);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

export function dayLabel(ms: number, now = Date.now()): string {
  if (dayKey(ms) === dayKey(now)) return "Today";
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  if (dayKey(ms) === dayKey(yesterday.getTime())) return "Yesterday";
  const date = new Date(ms);
  const sameYear = date.getFullYear() === new Date(now).getFullYear();
  return date.toLocaleDateString(undefined, sameYear ? { weekday: "long", month: "short", day: "numeric" } : { month: "short", day: "numeric", year: "numeric" });
}

/** Newest day first; calls inside a day newest first. */
export function groupByDay<T extends { started_at_ms: number }>(items: T[], now = Date.now()): DayGroup<T>[] {
  const groups = new Map<string, DayGroup<T>>();
  for (const item of [...items].sort((a, b) => b.started_at_ms - a.started_at_ms)) {
    const key = dayKey(item.started_at_ms);
    const group = groups.get(key) ?? { key, label: dayLabel(item.started_at_ms, now), items: [] };
    group.items.push(item);
    groups.set(key, group);
  }
  return [...groups.values()];
}

/** A finished call shorter than ten seconds is an accidental start. Running calls always count. */
export function isShort(session: { duration_ms: number | null; ended_at_ms: number | null }): boolean {
  return session.ended_at_ms !== null && (session.duration_ms ?? 0) < SHORT_MS;
}
