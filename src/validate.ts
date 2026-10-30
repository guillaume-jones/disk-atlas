type Fields = Readonly<Record<string, unknown>>;
type Tuple<N extends number, T extends number[] = []> = T["length"] extends N
  ? T
  : Tuple<N, [...T, number]>;

const describe = (value: unknown) => JSON.stringify(value)?.slice(0, 60) ?? String(value);
const fail = (expected: string, value: unknown) =>
  new TypeError(`Expected ${expected}, got ${describe(value)}`);

const isFields = (value: unknown): value is Fields =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isTuple = <N extends number>(value: unknown, length: N): value is Tuple<N> =>
  Array.isArray(value) && value.length === length && value.every(Number.isFinite);

export function object(value: unknown): Fields {
  if (!isFields(value)) throw fail("an object", value);
  return value;
}

export function number(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value)) throw fail("a number", value);
  return value;
}

export function boolean(value: unknown): boolean {
  if (typeof value !== "boolean") throw fail("a boolean", value);
  return value;
}

export function string(value: unknown): string {
  if (typeof value !== "string") throw fail("a string", value);
  return value;
}

export function nullable<T>(value: unknown, parse: (value: unknown) => T): T | null {
  return value === null ? null : parse(value);
}

export function array<T>(value: unknown, parse: (value: unknown) => T): T[] {
  if (!Array.isArray(value)) throw fail("an array", value);
  return value.map(parse);
}

export function numbers<N extends number>(value: unknown, length: N): Tuple<N> {
  if (!isTuple(value, length)) throw fail(`${length} numbers`, value);
  return value;
}

export function literal<T extends string>(value: unknown, options: readonly T[]): T {
  const match = options.find((option) => option === value);
  if (match === undefined) throw fail(`one of ${options.join(", ")}`, value);
  return match;
}
