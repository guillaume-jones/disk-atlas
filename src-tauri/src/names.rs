use crate::util::Rng;

const CONSONANTS: &[&[&str]] = &[
    &["p", "t", "k", "m", "n", "l", "s", "r"],
    &["b", "d", "g", "m", "n", "l", "r", "v", "z"],
    &["t", "k", "sh", "m", "n", "r", "h", "y"],
    &["th", "d", "l", "r", "n", "v", "g", "m", "s"],
    &["k", "kh", "t", "z", "r", "n", "b", "d", "g"],
    &["f", "v", "l", "r", "m", "n", "s", "t", "c"],
    &["q", "k", "t", "m", "n", "r", "s", "l", "y", "w"],
];
const VOWELS: &[&[&str]] = &[
    &["a", "e", "i", "o", "u"],
    &["a", "i", "u"],
    &["a", "e", "o", "ae", "ia"],
    &["e", "i", "o", "y", "ei"],
    &["a", "o", "u", "au", "ou"],
];
const FINALS: &[&[&str]] = &[&["n", "r", "l", "s"], &["m", "n", "th"], &["k", "t", "r"], &["n", "l", "sh", "x"], &["n", "r"]];
const SHAPES: &[&[&str]] = &[&["CV", "CVF", "V"], &["CV", "CVF"], &["CV", "VF", "CVF"], &["CV", "CV", "CVF"]];
const SUFFIXES: &[&[&str]] = &[
    &["ia", "or", "an"],
    &["eth", "ium", "os"],
    &["ar", "en", "is"],
    &["a", "ul", "heim"],
    &["ara", "oth", "und"],
    &["ane", "orn", "ys"],
];

/// A naming "language": a consistent phoneme set so all names on one continent sound related.
pub struct Lang {
    cons: &'static [&'static str],
    vowels: &'static [&'static str],
    finals: &'static [&'static str],
    shapes: &'static [&'static str],
    suffixes: &'static [&'static str],
    max_syl: usize,
}

impl Lang {
    pub fn new(seed: u64) -> Self {
        let mut r = Rng::new(seed);
        Lang {
            cons: r.pick(CONSONANTS),
            vowels: r.pick(VOWELS),
            finals: r.pick(FINALS),
            shapes: r.pick(SHAPES),
            suffixes: r.pick(SUFFIXES),
            max_syl: 2 + r.below(2),
        }
    }

    pub fn word(&self, rng: &mut Rng) -> String {
        loop {
            let syllables = 2 + rng.below(self.max_syl - 1);
            let mut w = String::new();
            for _ in 0..syllables {
                for ch in rng.pick(self.shapes).chars() {
                    w.push_str(match ch {
                        'C' => rng.pick(self.cons),
                        'V' => rng.pick(self.vowels),
                        _ => rng.pick(self.finals),
                    });
                }
            }
            if rng.f64() < 0.35 {
                w.push_str(rng.pick(self.suffixes));
            }
            if (4..=11).contains(&w.len()) && readable(&w) {
                let mut c = w.chars();
                return c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
            }
        }
    }
}

fn readable(w: &str) -> bool {
    let b = w.as_bytes();
    let vowel = |c: u8| b"aeiouy".contains(&c);
    !b.windows(3).any(|t| (t[0] == t[1] && t[1] == t[2]) || t.iter().all(|&c| vowel(c)) || t.iter().all(|&c| !vowel(c)))
}
