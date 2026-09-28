//! Search box grammar. Free text plus Gmail-style operators:
//!
//! - `from:sarah` / `from:me`: who sent it
//! - `in:family` / `with:mike`: which conversation (group name or participant)
//! - `has:link|photo|video|audio|file|attachment`
//! - `before:2024-03-01` / `after:2023` / `during:2024-06`
//! - `"exact phrase"`, and `-word` to exclude
//!
//! Values with spaces are quoted: `from:"sarah chen"`. Unknown `foo:bar`
//! stays free text so URLs and times ("10:30") search normally.

use serde::{Deserialize, Serialize};

use crate::tz::Tz;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HasFilter {
    Link,
    Photo,
    Video,
    Audio,
    File,
    Attachment,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedQuery {
    /// Plain words, in order (for FTS and for embedding).
    pub words: Vec<String>,
    /// Quoted phrases.
    pub phrases: Vec<String>,
    /// `-word` exclusions.
    pub excluded: Vec<String>,
    /// `from:` values; "me" means sent by me.
    pub from: Vec<String>,
    /// `in:` / `with:` values.
    pub chat: Vec<String>,
    pub has: Vec<HasFilter>,
    /// Inclusive lower bound, unix ms (local midnight of the date given).
    pub after_ms: Option<i64>,
    /// Exclusive upper bound, unix ms.
    pub before_ms: Option<i64>,
}

impl ParsedQuery {
    /// The free-text part, as typed (words and phrases), for embedding.
    pub fn free_text(&self) -> String {
        let mut parts: Vec<&str> = self.words.iter().map(String::as_str).collect();
        parts.extend(self.phrases.iter().map(String::as_str));
        parts.join(" ")
    }

    pub fn has_text(&self) -> bool {
        !self.words.is_empty() || !self.phrases.is_empty()
    }

    pub fn has_filters(&self) -> bool {
        !self.from.is_empty()
            || !self.chat.is_empty()
            || !self.has.is_empty()
            || self.after_ms.is_some()
            || self.before_ms.is_some()
    }

    /// FTS5 MATCH expression, or None when there's no free text.
    /// Every word is a prefix match so results appear while typing.
    pub fn fts_expression(&self) -> Option<String> {
        let mut terms: Vec<String> = Vec::new();
        for w in &self.words {
            for tok in fts_tokens(w) {
                // One-letter prefixes match most of the vocabulary and have no
                // prefix index: match those exactly.
                if tok.chars().count() == 1 {
                    terms.push(format!("\"{tok}\""));
                } else {
                    terms.push(format!("\"{tok}\"*"));
                }
            }
        }
        for p in &self.phrases {
            let toks = fts_tokens(p);
            if !toks.is_empty() {
                terms.push(format!("\"{}\"", toks.join(" ")));
            }
        }
        if terms.is_empty() {
            return None;
        }
        let mut expr = terms.join(" AND ");
        for x in &self.excluded {
            for tok in fts_tokens(x) {
                expr.push_str(&format!(" NOT \"{tok}\""));
            }
        }
        Some(expr)
    }
}

/// Words that carry no meaning in a natural-language question ("what's the
/// code for the gate"). Only dropped by the relaxed any-word fallback.
const STOPWORDS: &[&str] = &[
    "a", "am", "an", "and", "are", "as", "at", "be", "can", "could", "did", "do", "does", "for",
    "from", "get", "had", "has", "have", "how", "i", "if", "in", "is", "it", "its", "me", "my",
    "of", "on", "or", "s", "should", "so", "that", "the", "their", "there", "they", "this", "to",
    "was", "we", "were", "what", "whats", "when", "where", "which", "who", "why", "will", "with",
    "would", "you", "your",
];

/// A filler word of a natural-language question.
pub fn is_stopword(token: &str) -> bool {
    STOPWORDS.contains(&token)
}

impl ParsedQuery {
    /// Relaxed FTS5 expression: any meaningful word (stopwords dropped),
    /// for when requiring every word finds little. None if nothing is left
    /// or it would equal the strict expression.
    pub fn fts_expression_any(&self) -> Option<String> {
        let mut terms: Vec<String> = Vec::new();
        for w in self.words.iter().chain(self.phrases.iter()) {
            for tok in fts_tokens(w) {
                if STOPWORDS.contains(&tok.as_str()) || tok.chars().count() < 2 {
                    continue;
                }
                let t = format!("\"{tok}\"*");
                if !terms.contains(&t) {
                    terms.push(t);
                }
            }
        }
        let total_tokens: usize = self
            .words
            .iter()
            .chain(self.phrases.iter())
            .map(|w| fts_tokens(w).len())
            .sum();
        if terms.is_empty() || (terms.len() == 1 && total_tokens == 1) {
            return None;
        }
        let mut expr = format!("({})", terms.join(" OR "));
        for x in &self.excluded {
            for tok in fts_tokens(x) {
                expr.push_str(&format!(" NOT \"{tok}\""));
            }
        }
        Some(expr)
    }
}

/// Split into the tokens FTS5's unicode61 tokenizer would produce (letters
/// and digits), so user punctuation can never break the MATCH syntax.
fn fts_tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Parse the search box. `tz` turns dates into instants (local midnight).
pub fn parse(input: &str, tz: impl Into<Tz>) -> ParsedQuery {
    let tz = tz.into();
    let mut q = ParsedQuery::default();
    for tok in tokenize(input) {
        match tok {
            Tok::Phrase(p) => {
                if !p.trim().is_empty() {
                    q.phrases.push(p);
                }
            }
            Tok::Word(w) => apply_word(&mut q, &w),
            Tok::Op(key, value) => {
                if !apply_op(&mut q, &key, &value, tz) {
                    // Not an operator we know: search it as text.
                    apply_word(&mut q, &format!("{key}:{value}"));
                }
            }
        }
    }
    q
}

fn apply_word(q: &mut ParsedQuery, w: &str) {
    if let Some(rest) = w.strip_prefix('-') {
        if !rest.is_empty() {
            q.excluded.push(rest.to_string());
        }
        return;
    }
    q.words.push(w.to_string());
}

fn apply_op(q: &mut ParsedQuery, key: &str, value: &str, tz: Tz) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    match key.to_ascii_lowercase().as_str() {
        "from" => q.from.push(value.to_string()),
        "in" | "with" | "to" => q.chat.push(value.to_string()),
        "has" => {
            let f = match value.to_ascii_lowercase().as_str() {
                "link" | "links" | "url" => HasFilter::Link,
                "photo" | "photos" | "image" | "images" | "pic" | "picture" => HasFilter::Photo,
                "video" | "videos" => HasFilter::Video,
                "audio" | "voice" | "voicememo" => HasFilter::Audio,
                "file" | "files" | "doc" | "pdf" => HasFilter::File,
                "attachment" | "attachments" => HasFilter::Attachment,
                _ => return false,
            };
            if !q.has.contains(&f) {
                q.has.push(f);
            }
        }
        "before" => match date_range(value, tz) {
            Some((start, _)) => q.before_ms = Some(start),
            None => return false,
        },
        "after" => match date_range(value, tz) {
            Some((_, end)) => q.after_ms = Some(end),
            None => return false,
        },
        "during" | "on" | "in_year" => match date_range(value, tz) {
            Some((start, end)) => {
                q.after_ms = Some(start);
                q.before_ms = Some(end);
            }
            None => return false,
        },
        _ => return false,
    }
    true
}

/// `YYYY`, `YYYY-MM` or `YYYY-MM-DD` → [start, end) in unix ms, local time.
pub fn date_range(s: &str, tz: impl Into<Tz>) -> Option<(i64, i64)> {
    let tz = tz.into();
    let parts: Vec<&str> = s.split(['-', '/']).collect();
    let y: i64 = parts.first()?.parse().ok()?;
    if !(1990..=2200).contains(&y) {
        return None;
    }
    let (start, end) = match parts.len() {
        1 => ((y, 1, 1), (y + 1, 1, 1)),
        2 => {
            let m: i64 = parts[1].parse().ok()?;
            if !(1..=12).contains(&m) {
                return None;
            }
            (
                (y, m, 1),
                if m == 12 {
                    (y + 1, 1, 1)
                } else {
                    (y, m + 1, 1)
                },
            )
        }
        3 => {
            let m: i64 = parts[1].parse().ok()?;
            let d: i64 = parts[2].parse().ok()?;
            // The day after, via day numbers (month/year rollover).
            let (ny, nm, nd) = civil_next_day(y, m, d)?;
            ((y, m, d), (ny, nm, nd))
        }
        _ => return None,
    };
    Some((
        tz.local_midnight_ms(start.0, start.1, start.2)?,
        tz.local_midnight_ms(end.0, end.1, end.2)?,
    ))
}

fn civil_next_day(y: i64, m: i64, d: i64) -> Option<(i64, i64, i64)> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let next = chrono::NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32)?.succ_opt()?;
    use chrono::Datelike;
    Some((
        i64::from(next.year()),
        i64::from(next.month()),
        i64::from(next.day()),
    ))
}

enum Tok {
    Word(String),
    Phrase(String),
    Op(String, String),
}

fn tokenize(input: &str) -> Vec<Tok> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i] == '"' {
            let (s, next) = read_quoted(&chars, i + 1);
            out.push(Tok::Phrase(s));
            i = next;
            continue;
        }
        // A bare word, possibly `key:value` or `key:"quoted value"`.
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ':' && chars[i] != '"' {
            i += 1;
        }
        let head: String = chars[start..i].iter().collect();
        let is_key = !head.is_empty() && head.chars().all(|c| c.is_ascii_alphabetic() || c == '_');
        if i < chars.len() && chars[i] == ':' && is_key {
            i += 1;
            if i < chars.len() && chars[i] == '"' {
                let (v, next) = read_quoted(&chars, i + 1);
                out.push(Tok::Op(head, v));
                i = next;
            } else {
                let vs = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                let v: String = chars[vs..i].iter().collect();
                out.push(Tok::Op(head, v));
            }
            continue;
        }
        // Not an operator: consume the rest of the word (including ':' / '"').
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let w: String = chars[start..i].iter().collect();
        out.push(Tok::Word(w));
    }
    out
}

fn read_quoted(chars: &[char], mut i: usize) -> (String, usize) {
    let start = i;
    while i < chars.len() && chars[i] != '"' {
        i += 1;
    }
    let s: String = chars[start..i].iter().collect();
    (s, (i + 1).min(chars.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_and_text() {
        let q = parse(r#"gate code from:"sarah chen" in:family has:photo -old"#, 0);
        assert_eq!(q.words, vec!["gate", "code"]);
        assert_eq!(q.from, vec!["sarah chen"]);
        assert_eq!(q.chat, vec!["family"]);
        assert_eq!(q.has, vec![HasFilter::Photo]);
        assert_eq!(q.excluded, vec!["old"]);
        assert_eq!(
            q.fts_expression().unwrap(),
            r#""gate"* AND "code"* NOT "old""#
        );
    }

    #[test]
    fn unknown_operators_and_times_stay_text() {
        let q = parse("meet at 10:30 https://example.com/x foo:bar", 0);
        assert_eq!(
            q.words,
            vec!["meet", "at", "10:30", "https://example.com/x", "foo:bar"]
        );
        assert!(!q.has_filters());
    }

    #[test]
    fn phrases_and_punctuation_are_safe() {
        let q = parse(r#""see you" it's (fine) AND"#, 0);
        let e = q.fts_expression().unwrap();
        assert_eq!(e, r#""it"* AND "s" AND "fine"* AND "and"* AND "see you""#);
    }

    #[test]
    fn dates() {
        let q = parse("during:2024-02", 0);
        assert_eq!(q.after_ms, Some(1_706_745_600_000)); // 2024-02-01 UTC
        assert_eq!(q.before_ms, Some(1_709_251_200_000)); // 2024-03-01 UTC
        let q = parse("after:2023-12-31 before:2024", -5 * 3600);
        assert_eq!(q.after_ms, Some((1_704_067_200 + 5 * 3600) * 1000)); // 2024-01-01 local
        assert_eq!(q.before_ms, Some((1_704_067_200 + 5 * 3600) * 1000));
        assert!(parse("before:banana", 0)
            .words
            .contains(&"before:banana".to_string()));
    }

    #[test]
    fn relaxed_expression() {
        let q = parse("what's the code to get in the gate -old", 0);
        assert_eq!(
            q.fts_expression_any().unwrap(),
            r#"("code"* OR "gate"*) NOT "old""#
        );
        assert_eq!(
            parse("UA 1423", 0).fts_expression_any().unwrap(),
            r#"("ua"* OR "1423"*)"#
        );
        // A single word has no relaxed form.
        assert!(parse("gate", 0).fts_expression_any().is_none());
        assert!(parse("what is the", 0).fts_expression_any().is_none());
    }

    #[test]
    fn empty_text_with_filters() {
        let q = parse("from:me has:link", 0);
        assert!(q.fts_expression().is_none());
        assert!(q.has_filters());
    }
}
