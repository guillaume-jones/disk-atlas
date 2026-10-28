export interface RecencyBand {
  label: string;
  /** Old-atlas hypsometric tint. */
  color: string;
  /** Data modified more recently than this many days rises into this band; null for the oldest. */
  newerThanDays: number | null;
}

/** Oldest (lowland) to newest (highland). */
export const RECENCY_BANDS: readonly RecencyBand[] = [
  { label: "Over 2 years", color: "#9eb489", newerThanDays: null },
  { label: "1 to 2 years", color: "#abbd92", newerThanDays: 730 },
  { label: "6 to 12 months", color: "#c1cd9f", newerThanDays: 365 },
  { label: "3 to 6 months", color: "#d9daab", newerThanDays: 180 },
  { label: "6 weeks to 3 months", color: "#ede3b3", newerThanDays: 90 },
  { label: "3 to 6 weeks", color: "#e8ce9e", newerThanDays: 45 },
  { label: "1 to 3 weeks", color: "#d9b285", newerThanDays: 21 },
  { label: "1 to 7 days", color: "#c29571", newerThanDays: 7 },
  { label: "Within a day", color: "#a57d61", newerThanDays: 1 },
];

/** Mean land elevation the Rust generator produces for a given age: base plus average ridge contribution. */
function elevationFor(days: number): number {
  const fast = Math.exp(-days / 60);
  const tail = Math.exp(-days / 400);
  return 0.075 + 0.71 * (0.85 * fast + 0.15 * tail);
}

/** Elevations where each band after the oldest begins. */
export const BAND_EDGES = RECENCY_BANDS.flatMap(({ newerThanDays }) =>
  newerThanDays === null ? [] : [elevationFor(newerThanDays)],
);

export function rgb(hex: string): [number, number, number] {
  const channel = (at: number) => parseInt(hex.slice(at, at + 2), 16) / 255;
  return [channel(1), channel(3), channel(5)];
}
