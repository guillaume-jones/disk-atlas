const SECONDS_PER_DAY = 86_400;
const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"];

export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < BYTE_UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  const digits = value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value);
  return `${digits} ${BYTE_UNITS[unit]}`;
}

/** `then` and `now` are Unix seconds. */
export function formatAge(then: number, now: number): string {
  if (!then) return "never";
  const days = (now - then) / SECONDS_PER_DAY;
  if (days < 1) return "today";
  if (days < 2) return "yesterday";
  if (days < 45) return `${Math.round(days)} days ago`;
  if (days < 540) return `${Math.round(days / 30)} months ago`;
  return `${(days / 365).toFixed(1)} years ago`;
}

export const homeRelative = (path: string, home: string) =>
  home && path.startsWith(home) ? "~" + path.slice(home.length) : path;
