import type { Kind, Region } from "./api";

/** Phoneme tables a language samples from; each entry is one coherent set. */
interface Phonology {
  consonants: readonly (readonly string[])[];
  vowels: readonly (readonly string[])[];
  finals: readonly (readonly string[])[];
  /** Syllable shapes: C = consonant, V = vowel, F = final. */
  shapes: readonly (readonly string[])[];
  suffixes: readonly (readonly string[])[];
}

/** A theme's naming conventions. Patterns hold `{}` for the folder's word or an invented one. */
export interface Vocabulary {
  /** Fixed names of regions that aren't folders. */
  landmarks: Partial<Record<Kind, string>>;
  continent: readonly string[];
  island: readonly string[];
  nation: readonly string[];
  province: readonly string[];
  locale: readonly string[];
  glacier: readonly string[];
  barrens: readonly string[];
  freeCity: readonly string[];
  city: readonly string[];
  outpost: readonly string[];
  qualifiers: readonly string[];
  phonology: Phonology;
  /** Chance a region ignores its folder's word for an invented one. */
  invention: number;
}

/** Attempts with the folder's own word before falling back to invented ones. */
const ATTEMPTS_PER_WORD = 20;
const VOWELS = "aeiouy";
const SUFFIX_CHANCE = 0.35;
const MIN_WORD_LETTERS = 4;
const MAX_WORD_LETTERS = 11;

/** Mulberry32: every name is seeded from its region's hash, so a survey keeps its names in each theme. */
class Rng {
  constructor(private state: number) {}

  f64(): number {
    this.state = (this.state + 0x6d2b79f5) | 0;
    let t = Math.imul(this.state ^ (this.state >>> 15), 1 | this.state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 2 ** 32;
  }

  below(n: number): number {
    return Math.floor(this.f64() * n);
  }

  pick<T>(items: readonly T[]): T {
    const item = items[this.below(items.length)];
    if (item === undefined) throw new RangeError("Nothing to pick from");
    return item;
  }
}

const isVowel = (letter: string) => VOWELS.includes(letter);

/** No triple letters, and no three vowels or three consonants in a row. */
function isReadable(word: string): boolean {
  for (let i = 2; i < word.length; i++) {
    const [a = "", b = "", c = ""] = word.slice(i - 2, i + 1);
    if ((a === b && b === c) || (isVowel(a) && isVowel(b) && isVowel(c))) return false;
    if (!isVowel(a) && !isVowel(b) && !isVowel(c)) return false;
  }
  return true;
}

/** Fill a pattern; suffixes fuse with the word ("Cache" + "{}ia" → "Cachia"). */
function fill(pattern: string, word: string): string {
  const suffix = pattern.startsWith("{}") ? pattern.slice(2) : "";
  if (!/^[a-z]/.test(suffix)) return pattern.replace("{}", word);
  const elides = isVowel(suffix.charAt(0)) && isVowel(word.slice(-1));
  return (elides ? word.slice(0, -1) : word) + suffix;
}

/** A naming language: a consistent phoneme set, so all invented names on one continent sound related. */
class Lang {
  private readonly consonants: readonly string[];
  private readonly vowels: readonly string[];
  private readonly finals: readonly string[];
  private readonly shapes: readonly string[];
  private readonly suffixes: readonly string[];
  private readonly maxSyllables: number;

  constructor(seed: number, phonology: Phonology) {
    const rng = new Rng(seed);
    this.consonants = rng.pick(phonology.consonants);
    this.vowels = rng.pick(phonology.vowels);
    this.finals = rng.pick(phonology.finals);
    this.shapes = rng.pick(phonology.shapes);
    this.suffixes = rng.pick(phonology.suffixes);
    this.maxSyllables = 2 + rng.below(2);
  }

  word(rng: Rng): string {
    for (;;) {
      const syllables = 2 + rng.below(this.maxSyllables - 1);
      let word = "";
      for (let i = 0; i < syllables; i++) {
        for (const slot of rng.pick(this.shapes)) {
          const letters =
            slot === "C" ? this.consonants : slot === "V" ? this.vowels : this.finals;
          word += rng.pick(letters);
        }
      }
      if (rng.f64() < SUFFIX_CHANCE) word += rng.pick(this.suffixes);
      if (word.length >= MIN_WORD_LETTERS && word.length <= MAX_WORD_LETTERS && isReadable(word))
        return word.charAt(0).toUpperCase() + word.slice(1);
    }
  }
}

function patternsFor(region: Region, polar: boolean, vocabulary: Vocabulary): readonly string[] {
  if (region.kind === "continent") return vocabulary.continent;
  if (region.kind === "island") return vocabulary.island;
  if (region.kind === "bundle") return vocabulary.freeCity;
  if (region.kind === "file") return vocabulary.barrens;
  if (polar) return vocabulary.glacier;
  if (region.depth <= 2) return vocabulary.nation;
  return region.depth === 3 ? vocabulary.province : vocabulary.locale;
}

/** Names every region and city in `regions`, which run parents before children. */
export function nameRegions(regions: readonly Region[], vocabulary: Vocabulary) {
  const { landmarks, phonology, qualifiers, invention } = vocabulary;
  const used = new Set(Object.values(landmarks));
  // A map-wide unique name: `word` (or an invented one) in the first free pattern, then with qualifiers like "Upper".
  const claim = (word: string | null, patterns: readonly string[], lang: Lang, rng: Rng) => {
    const own = word && rng.f64() >= invention ? word : null;
    const start = rng.below(patterns.length);
    for (let attempt = 0; attempt < 2 * ATTEMPTS_PER_WORD; attempt++) {
      const stem = own !== null && attempt < ATTEMPTS_PER_WORD ? own : lang.word(rng);
      const pattern = patterns[(start + attempt) % patterns.length] ?? "{}";
      const base = fill(pattern, stem);
      const round = attempt % ATTEMPTS_PER_WORD;
      const name =
        round < patterns.length ? base : `${qualifiers[round % qualifiers.length] ?? ""} ${base}`;
      if (!used.has(name)) {
        used.add(name);
        return name;
      }
    }
    return `${lang.word(rng)} ${rng.below(1000)}`;
  };
  const continents = regions.filter(({ kind }) => kind === "continent");
  const langs = new Map<number, Lang>();
  const polar = new Set<number>();
  for (const region of regions) {
    if (region.kind === "polar" || polar.has(region.parent)) polar.add(region.id);
    if (region.depth === 1) {
      // Each continent speaks its own language; islands borrow one.
      const speaker =
        region.kind === "continent" ? region : continents[region.hash % continents.length];
      langs.set(region.id, new Lang((speaker ?? region).hash, phonology));
    }
    const lang = langs.get(region.id) ?? langs.get(region.parent);
    const landmark = landmarks[region.kind];
    if (landmark !== undefined || !lang) {
      region.name = landmark ?? "";
      region.city = null;
      continue;
    }
    langs.set(region.id, lang);
    const rng = new Rng(region.hash);
    const inIce = polar.has(region.id);
    region.name = claim(region.word, patternsFor(region, inIce, vocabulary), lang, rng);
    const patterns = inIce ? vocabulary.outpost : vocabulary.city;
    region.city = region.settled ? claim(region.word, patterns, lang, rng) : null;
  }
}
