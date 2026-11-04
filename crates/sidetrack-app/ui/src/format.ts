const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** Same units and rounding as the CLI (`format_bytes` in sidetrack-core). */
export function bytes(n: number): string {
  let v = n;
  let unit = 0;
  while (v >= 1024 && unit < UNITS.length - 1) {
    v /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${n} B` : `${v.toFixed(1)} ${UNITS[unit]}`;
}

/** Just the number of GB, for the large figures. */
export const gb = (n: number) => (n / 1024 ** 3).toFixed(1);

export function seconds(s: number): string {
  return s < 10 ? `${s.toFixed(1)} s` : `${Math.round(s)} s`;
}

/** A unix time as "3 h ago", "6 days ago", or a date for anything older than a month. */
export function ago(t: number): string {
  const s = Date.now() / 1000 - t;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  if (s < 30 * 86400) return plural(Math.floor(s / 86400), "day") + " ago";
  return new Date(t * 1000).toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
}

export const plural =(n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
