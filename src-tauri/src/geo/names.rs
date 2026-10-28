//! Place names: the most distinctive word of a folder, dressed in a pattern ("Kingdom of {}"), or an invented word
//! in a continent's own language when the folder has none.
use super::RegionKind;
use crate::rng::Rng;
use crate::scan::BUNDLE_EXTENSIONS;
use std::borrow::Cow;
use std::collections::HashSet;

const CONTINENT: &[&str] = &["{}land", "{}ia", "Greater {}"];
const ISLAND: &[&str] = &["Isle of {}", "{} Island", "The {} Isles"];
const NATION: &[&str] = &["Kingdom of {}", "{}stan", "Republic of {}", "Duchy of {}", "{}mark"];
const PROVINCE: &[&str] = &["{}shire", "{} Province", "March of {}", "{} Vale"];
const LOCALE: &[&str] = &["{} Downs", "{} Reach", "{} Fells", "{} Heath", "{} Hollow", "{} Weald"];
const GLACIER: &[&str] = &["{} Glacier", "{} Icefield", "{} Shelf"];
const BARRENS: &[&str] = &["{} Barrens", "{} Expanse", "{} Plateau"];
const FREE_CITY: &[&str] = &["Free City of {}", "{} Free City"];
const CITY: &[&str] = &[
    "{}ton", "{}burg", "Port {}", "{}ville", "{}ham", "Fort {}", "{}ford", "{}field", "{}stead", "{}minster", "{}holm", "{}wick", "{}by",
    "{}mouth", "{}dale", "{}haven", "Saint {}", "{}polis", "{}grad", "Mount {}", "{}bridge", "{}port",
];
const OUTPOST: &[&str] = &["Fort {}", "{} Base", "{} Station", "Camp {}", "Outpost {}"];
const QUALIFIERS: &[&str] = &["Upper", "Lower", "North", "South", "East", "West", "New", "Old", "Far", "Little"];
/// Attempts with the folder's own word before falling back to invented ones.
const ATTEMPTS_PER_WORD: usize = 20;

/// Folder names that say nothing about their contents.
const GENERIC: &[&str] = &[
    "data", "library", "contents", "resources", "caches", "cache", "src", "lib", "bin", "files", "tmp", "temp", "default",
    "application support", "containers", "group containers", "users", "private", "var", "folders", "shared", "local", "share", "store",
    "storage", "blobs", "objects", "current", "latest", "versions", "t", "c", "x", "0", "documents and settings",
];
/// Tokens that never make a place name on their own.
const STOP: &[&str] = &[
    "com", "org", "net", "www", "the", "and", "for", "this", "that", "with", "from", "into", "apple", "google", "microsoft", "mozilla",
    "app", "apps", "lib", "libs", "src", "tmp", "temp", "data", "file", "files", "default", "stable", "latest", "current", "main",
    "master", "darwin", "users", "user",
];
const VOWELS: &str = "aeiouy";

pub fn region_patterns(kind: RegionKind, depth: u8, polar: bool) -> &'static [&'static str] {
    match kind {
        RegionKind::Continent => CONTINENT,
        RegionKind::Island => ISLAND,
        RegionKind::Bundle => FREE_CITY,
        RegionKind::File => BARRENS,
        _ if polar => GLACIER,
        _ if depth <= 2 => NATION,
        _ if depth == 3 => PROVINCE,
        _ => LOCALE,
    }
}

pub fn city_patterns(polar: bool) -> &'static [&'static str] {
    if polar { OUTPOST } else { CITY }
}

/// Hands out names that are unique across the whole map.
#[derive(Default)]
pub struct Gazetteer {
    used: HashSet<String>,
}

impl Gazetteer {
    pub fn reserve(&mut self, name: &str) {
        self.used.insert(name.into());
    }

    /// `word` (or an invented one) in the first free pattern, then with qualifiers like "Upper".
    pub fn claim(&mut self, word: Option<&str>, patterns: &[&str], lang: Lang, rng: &mut Rng) -> String {
        let start = rng.below(patterns.len());
        for attempt in 0..2 * ATTEMPTS_PER_WORD {
            let word = match word {
                Some(w) if attempt < ATTEMPTS_PER_WORD => Cow::Borrowed(w),
                _ => Cow::Owned(lang.word(rng)),
            };
            let base = fill(patterns[(start + attempt) % patterns.len()], &word);
            let round = attempt % ATTEMPTS_PER_WORD;
            let name = if round < patterns.len() {
                base
            } else {
                format!("{} {base}", QUALIFIERS[round % QUALIFIERS.len()])
            };
            if self.used.insert(name.clone()) {
                return name;
            }
        }
        format!("{} {}", lang.word(rng), rng.below(1000))
    }
}

/// Fill a pattern; suffixes fuse with the word ("Cache" + "{}ia" → "Cachia").
fn fill(pattern: &str, word: &str) -> String {
    match pattern.strip_prefix("{}") {
        Some(suffix) if suffix.starts_with(|c: char| c.is_ascii_lowercase()) => {
            let vowel = |c: char| VOWELS.contains(c);
            let stem = if suffix.starts_with(vowel) && word.ends_with(vowel) {
                &word[..word.len() - 1]
            } else {
                word
            };
            format!("{stem}{suffix}")
        }
        _ => pattern.replace("{}", word),
    }
}

/// The most meaningful real name in a collapsed chain of folders: the deepest non-generic one.
pub fn informative(chain: &[String]) -> String {
    chain
        .iter()
        .rev()
        .find(|n| !is_generic(n))
        .or(chain.last())
        .cloned()
        .unwrap_or_default()
}

fn is_generic(name: &str) -> bool {
    let is_hex_id = name.len() >= 16 && name.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    let is_version = name.chars().all(|c| c.is_ascii_digit() || c == '.');
    GENERIC.contains(&name.to_lowercase().as_str()) || is_hex_id || is_version
}

/// The outermost meaningful word in a collapsed chain: `.pyenv/versions/3.14/lib` is Pyenv, not Version.
pub fn distinctive(chain: &[String]) -> Option<String> {
    chain.iter().find_map(|n| word_of(n))
}

/// The most distinctive word in a folder or file name, e.g. `com.apple.Safari` → Safari, `.gradle` → Gradle.
fn word_of(name: &str) -> Option<String> {
    if name == "node_modules" {
        return Some("Node".into());
    }
    let stem = match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && (BUNDLE_EXTENSIONS.iter().any(|b| b.eq_ignore_ascii_case(ext)) || is_file_extension(ext)) =>
        {
            stem
        }
        _ => name,
    };
    let mut best: Option<String> = None;
    for token in tokens(stem) {
        if token.chars().count() < 3 || !token.chars().all(char::is_alphabetic) {
            continue;
        }
        let lower = token.to_lowercase();
        if STOP.contains(&lower.as_str()) || !is_pronounceable(&lower) {
            continue;
        }
        let word = singular(&lower);
        if best.as_ref().is_none_or(|b| b.len() < word.len()) {
            best = Some(word);
        }
    }
    best.map(|w| capitalized(&w))
}

fn is_file_extension(ext: &str) -> bool {
    ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Splits on punctuation and camelCase humps.
fn tokens(stem: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for part in stem.split(|c: char| !c.is_alphanumeric()) {
        let mut current = String::new();
        let mut prev_lower = false;
        for ch in part.chars() {
            if ch.is_uppercase() && prev_lower {
                tokens.push(std::mem::take(&mut current));
            }
            prev_lower = ch.is_lowercase();
            current.push(ch);
        }
        tokens.push(current);
    }
    tokens
}

/// Rejects random identifiers like `ekpyefnfmdbiwhbx`: too long, or four consonants in a row. Short acronyms (npm, nvm) pass.
fn is_pronounceable(lower: &str) -> bool {
    let consonant_run = lower.split(|c| VOWELS.contains(c)).map(str::len).max().unwrap_or(0);
    lower.len() <= 4 || (lower.len() <= 16 && consonant_run < 4 && lower.chars().any(|c| VOWELS.contains(c)))
}

fn singular(word: &str) -> String {
    if word.len() > 4 && word.ends_with("ies") {
        format!("{}y", &word[..word.len() - 3])
    } else if word.len() > 4 && word.ends_with('s') && !["ss", "us", "is"].iter().any(|e| word.ends_with(e)) {
        word[..word.len() - 1].to_string()
    } else {
        word.to_string()
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

const CONSONANTS: &[&[&str]] = &[
    &["p", "t", "k", "m", "n", "l", "s", "r"],
    &["b", "d", "g", "m", "n", "l", "r", "v", "z"],
    &["t", "k", "sh", "m", "n", "r", "h", "y"],
    &["th", "d", "l", "r", "n", "v", "g", "m", "s"],
    &["k", "kh", "t", "z", "r", "n", "b", "d", "g"],
    &["f", "v", "l", "r", "m", "n", "s", "t", "c"],
    &["q", "k", "t", "m", "n", "r", "s", "l", "y", "w"],
];
const VOWEL_SETS: &[&[&str]] = &[
    &["a", "e", "i", "o", "u"],
    &["a", "i", "u"],
    &["a", "e", "o", "ae", "ia"],
    &["e", "i", "o", "y", "ei"],
    &["a", "o", "u", "au", "ou"],
];
const FINALS: &[&[&str]] = &[
    &["n", "r", "l", "s"],
    &["m", "n", "th"],
    &["k", "t", "r"],
    &["n", "l", "sh", "x"],
    &["n", "r"],
];
/// Syllable shapes: C = consonant, V = vowel, F = final.
const SHAPES: &[&[&str]] = &[&["CV", "CVF", "V"], &["CV", "CVF"], &["CV", "VF", "CVF"], &["CV", "CV", "CVF"]];
const SUFFIXES: &[&[&str]] = &[
    &["ia", "or", "an"],
    &["eth", "ium", "os"],
    &["ar", "en", "is"],
    &["a", "ul", "heim"],
    &["ara", "oth", "und"],
    &["ane", "orn", "ys"],
];
const SUFFIX_CHANCE: f64 = 0.35;
const WORD_LETTERS: std::ops::RangeInclusive<usize> = 4..=11;

/// A naming language: a consistent phoneme set so all invented names on one continent sound related.
#[derive(Clone, Copy)]
pub struct Lang {
    consonants: &'static [&'static str],
    vowels: &'static [&'static str],
    finals: &'static [&'static str],
    shapes: &'static [&'static str],
    suffixes: &'static [&'static str],
    max_syllables: usize,
}

impl Lang {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        Lang {
            consonants: rng.pick(CONSONANTS),
            vowels: rng.pick(VOWEL_SETS),
            finals: rng.pick(FINALS),
            shapes: rng.pick(SHAPES),
            suffixes: rng.pick(SUFFIXES),
            max_syllables: 2 + rng.below(2),
        }
    }

    pub fn word(&self, rng: &mut Rng) -> String {
        loop {
            let syllables = 2 + rng.below(self.max_syllables - 1);
            let mut word = String::new();
            for _ in 0..syllables {
                for slot in rng.pick(self.shapes).chars() {
                    word.push_str(match slot {
                        'C' => rng.pick(self.consonants),
                        'V' => rng.pick(self.vowels),
                        _ => rng.pick(self.finals),
                    });
                }
            }
            if rng.f64() < SUFFIX_CHANCE {
                word.push_str(rng.pick(self.suffixes));
            }
            if WORD_LETTERS.contains(&word.len()) && is_readable(&word) {
                return capitalized(&word);
            }
        }
    }
}

/// No triple letters, and no three vowels or three consonants in a row.
fn is_readable(word: &str) -> bool {
    let vowel = |c: u8| VOWELS.as_bytes().contains(&c);
    !word
        .as_bytes()
        .windows(3)
        .any(|t| (t[0] == t[1] && t[1] == t[2]) || t.iter().all(|&c| vowel(c)) || t.iter().all(|&c| !vowel(c)))
}
