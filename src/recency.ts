interface RecencyBand {
  label: string;
  /** Data modified more recently than this many days rises into this band; null for the oldest. */
  newerThanDays: number | null;
}

/** Oldest (lowland) to newest (highland). */
export const RECENCY_BANDS: readonly RecencyBand[] = [
  { label: "Over 2 years", newerThanDays: null },
  { label: "1 to 2 years", newerThanDays: 730 },
  { label: "6 to 12 months", newerThanDays: 365 },
  { label: "3 to 6 months", newerThanDays: 180 },
  { label: "6 weeks to 3 months", newerThanDays: 90 },
  { label: "3 to 6 weeks", newerThanDays: 45 },
  { label: "1 to 3 weeks", newerThanDays: 21 },
  { label: "1 to 7 days", newerThanDays: 7 },
  { label: "Within a day", newerThanDays: 1 },
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
