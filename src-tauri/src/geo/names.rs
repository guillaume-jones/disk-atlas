//! The words place names grow from: the most distinctive word of a folder, which the frontend dresses in a theme's
//! vocabulary.
use crate::scan::BUNDLE_EXTENSIONS;

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
