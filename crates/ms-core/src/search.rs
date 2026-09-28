//! Hybrid search: FTS5 keyword hits per message + semantic hits per
//! conversation window, fused with reciprocal rank fusion.

use std::collections::HashMap;
use std::time::Instant;

use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection, OptionalExtension};

use crate::query::{self, HasFilter, ParsedQuery};
use crate::store::{f32_blob, vec_table_exists, Store};
use crate::types::*;
use crate::Error;

/// Keyword matches scored, newest first. Message ids are chronological, so
/// FTS can stop early: a word in 100k messages only scores its newest ones,
/// keeping common words (and 1-2 letter prefixes while typing) fast.
const KEYWORD_SCAN: usize = 3000;
/// Keyword candidates considered before fusion.
const KEYWORD_POOL: usize = 300;
/// Below this many all-words matches, also look for any-word matches.
const RELAX_BELOW: usize = 20;
/// Semantic windows considered before fusion.
const SEMANTIC_POOL: usize = 60;
/// Reciprocal rank fusion constant.
const RRF_K: f64 = 60.0;
/// Windows worse than the embedder's `max_distance`, or this much worse than the best window for the query.
const SEMANTIC_RELATIVE_SLACK: f64 = 0.06;

/// Resolved `from:` people and `in:` chats (None = any chat).
type Filters = (FromFilter, Option<Vec<i64>>);

/// Who `from:` refers to.
#[derive(Default)]
struct FromFilter {
    me: bool,
    handles: Vec<i64>,
}

impl Store {
    /// Search the index. `embedder` enables meaning search; without it (or
    /// before any window is embedded) results are keyword-only.
    pub fn search(
        &self,
        input: &str,
        limit: usize,
        embedder: Option<&dyn Embedder>,
    ) -> Result<SearchResults, Error> {
        let started = Instant::now();
        let parsed = query::parse(input, self.tz());
        let limit = limit.clamp(1, 500);

        // Embed before taking the DB lock: it's the slow part (~10-30 ms).
        let query_vec = match (embedder, parsed.has_text()) {
            (Some(e), true) => e.embed_query(&parsed.free_text()).ok(),
            _ => None,
        };

        let conn = self.reader();
        let semantic_ready = query_vec.is_some()
            && vec_table_exists(&conn)?
            && conn
                .query_row("SELECT 1 FROM windows WHERE embedded=1 LIMIT 1", [], |_| {
                    Ok(())
                })
                .optional()?
                .is_some();

        let empty = |semantic| SearchResults {
            hits: Vec::new(),
            query: parsed.clone(),
            semantic,
            took_ms: started.elapsed().as_secs_f64() * 1000.0,
        };

        let Some(filters) = resolve_filters(&conn, &parsed)? else {
            // A from:/in: that matches nobody: nothing can match.
            return Ok(empty(semantic_ready));
        };
        let (where_sql, where_params) = filter_sql(&parsed, &filters);

        // Filter-only query: newest first.
        if !parsed.has_text() {
            if !parsed.has_filters() {
                return Ok(empty(semantic_ready));
            }
            // ?1 is unused here; filter placeholders are numbered from ?2.
            let sql = format!(
                "SELECT m.id FROM messages m WHERE ?1 IS NULL AND m.kind=0 {where_sql}
                 ORDER BY m.date_ms DESC LIMIT {limit}"
            );
            let mut p: Vec<Value> = vec![Value::Null];
            p.extend(where_params.iter().cloned());
            let ids: Vec<i64> = conn
                .prepare_cached(&sql)?
                .query_map(params_from_iter(p.iter()), |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            let ranked: Vec<Ranked> = ids
                .into_iter()
                .map(|id| Ranked {
                    id,
                    score: 0.0,
                    keyword: false,
                    meaning: false,
                })
                .collect();
            let hits = build_hits(&conn, ranked, &parsed, None, MatchedBy::Filter)?;
            return Ok(SearchResults {
                hits,
                query: parsed,
                semantic: semantic_ready,
                took_ms: started.elapsed().as_secs_f64() * 1000.0,
            });
        }

        // ---- keyword ----
        let now_ms = conn.query_row("SELECT MAX(date_ms) FROM messages", [], |r| {
            r.get::<_, Option<i64>>(0)
        })?;
        // Every word first; if that finds little (natural-language questions
        // rarely share every word with the answer), any meaningful word.
        let fts = parsed.fts_expression();
        let mut keyword: Vec<(i64, f64)> = Vec::new();
        let mut relaxed: Vec<(i64, f64)> = Vec::new();
        if let Some(expr) = &fts {
            keyword = keyword_matches(&conn, expr, &where_sql, &where_params, now_ms)?;
            if keyword.len() < RELAX_BELOW {
                if let Some(any) = parsed.fts_expression_any() {
                    let strict: std::collections::HashSet<i64> =
                        keyword.iter().map(|k| k.0).collect();
                    relaxed = keyword_matches(&conn, &any, &where_sql, &where_params, now_ms)?
                        .into_iter()
                        .filter(|k| !strict.contains(&k.0))
                        .collect();
                }
            }
        }

        // ---- meaning ----
        let mut semantic: Vec<i64> = Vec::new(); // anchor message ids, best first
        if semantic_ready {
            let qv = query_vec.as_ref().expect("semantic_ready implies a vector");
            let keyword_rank: HashMap<i64, usize> = keyword
                .iter()
                .chain(relaxed.iter())
                .enumerate()
                .map(|(i, k)| (k.0, i))
                .collect();
            semantic = semantic_anchors(
                &conn,
                qv,
                embedder.map_or(0.55, |e| e.max_distance()),
                &parsed,
                &where_sql,
                &where_params,
                &keyword_rank,
            )?;
        }

        // ---- fuse ----
        let mut fused: HashMap<i64, Ranked> = HashMap::new();
        // All-words and any-word matches are separate ranked lists. (Weighting
        // any-word matches down was tried; on the fixture's 33 questions every
        // weight from 0.35 to 1 scored within noise, so none is applied.)
        for list in [&keyword, &relaxed] {
            for (rank, (id, _)) in list.iter().enumerate() {
                let e = fused.entry(*id).or_insert(Ranked {
                    id: *id,
                    score: 0.0,
                    keyword: false,
                    meaning: false,
                });
                e.score += 1.0 / (RRF_K + rank as f64 + 1.0);
                e.keyword = true;
            }
        }
        for (rank, id) in semantic.iter().enumerate() {
            let e = fused.entry(*id).or_insert(Ranked {
                id: *id,
                score: 0.0,
                keyword: false,
                meaning: false,
            });
            e.score += 1.0 / (RRF_K + rank as f64 + 1.0);
            e.meaning = true;
        }
        let mut ranked: Vec<Ranked> = fused.into_values().collect();
        ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then(b.id.cmp(&a.id)));
        ranked.truncate(limit);

        let hits = build_hits(&conn, ranked, &parsed, fts.as_deref(), MatchedBy::Keyword)?;
        Ok(SearchResults {
            hits,
            query: parsed,
            semantic: semantic_ready,
            took_ms: started.elapsed().as_secs_f64() * 1000.0,
        })
    }
}

/// FTS matches for `expr`, best first: bm25 with mild recency, over the
/// newest `KEYWORD_SCAN` matches.
fn keyword_matches(
    conn: &Connection,
    expr: &str,
    where_sql: &str,
    where_params: &[Value],
    now_ms: Option<i64>,
) -> Result<Vec<(i64, f64)>, Error> {
    let sql = format!(
        "SELECT m.id, bm25(messages_fts), m.date_ms
         FROM messages_fts JOIN messages m ON m.id = messages_fts.rowid
         WHERE messages_fts MATCH ?1 AND m.kind=0 {where_sql}
         ORDER BY messages_fts.rowid DESC LIMIT {KEYWORD_SCAN}"
    );
    let mut p: Vec<Value> = vec![Value::Text(expr.to_string())];
    p.extend(where_params.iter().cloned());
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params_from_iter(p.iter()), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, f64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, bm25, date) = row?;
        // bm25 is negative (lower = better); blend in mild recency so
        // equally good matches show recent conversations first.
        let age_days = now_ms.map_or(0.0, |n| ((n - date).max(0) as f64) / 86_400_000.0);
        out.push((id, -bm25 * (0.75 + 0.25 * (-age_days / 730.0).exp())));
    }
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out.truncate(KEYWORD_POOL);
    Ok(out)
}

struct Ranked {
    id: i64,
    score: f64,
    keyword: bool,
    meaning: bool,
}

/// Nearest windows → one anchor message each, honoring all filters.
fn semantic_anchors(
    conn: &Connection,
    qv: &[f32],
    max_distance: f64,
    parsed: &ParsedQuery,
    where_sql: &str,
    where_params: &[Value],
    keyword_rank: &HashMap<i64, usize>,
) -> Result<Vec<i64>, Error> {
    // Date bounds can be pushed into the KNN query; the rest is post-filtered,
    // so over-fetch when filters are present.
    let k = if parsed.from.is_empty() && parsed.chat.is_empty() && parsed.has.is_empty() {
        SEMANTIC_POOL
    } else {
        SEMANTIC_POOL * 8
    };
    let mut sql = String::from(
        "SELECT rowid, distance FROM vec_windows
         WHERE embedding MATCH vec_quantize_int8(?1, 'unit') AND k = ?2",
    );
    let mut p: Vec<Value> = vec![Value::Blob(f32_blob(qv)), Value::Integer(k as i64)];
    if let Some(after) = parsed.after_ms {
        // A window starting up to one gap before `after` can still contain matches.
        sql.push_str(" AND start_ms >= ?3");
        p.push(Value::Integer(after - crate::store::WINDOW_GAP_MS * 4));
    }
    if let Some(before) = parsed.before_ms {
        sql.push_str(&format!(" AND start_ms < ?{}", p.len() + 1));
        p.push(Value::Integer(before));
    }
    sql.push_str(" ORDER BY distance");
    let windows: Vec<(i64, f64)> = conn
        .prepare_cached(&sql)?
        .query_map(params_from_iter(p.iter()), |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let Some(best) = windows.first().map(|w| w.1) else {
        return Ok(Vec::new());
    };
    let cutoff = max_distance.min(best + SEMANTIC_RELATIVE_SLACK);

    let words: Vec<String> = parsed
        .words
        .iter()
        .chain(parsed.phrases.iter())
        .map(|w| w.to_lowercase())
        .filter(|w| w.chars().count() >= 3)
        .collect();
    let cand_sql = format!(
        "SELECT m.id, m.text FROM windows w JOIN messages m ON {}
         WHERE w.id = ?1 AND m.kind=0 AND m.unsent=0 {where_sql} ORDER BY m.date_ms, m.id",
        Store::window_span_sql()
    );
    let mut cand = conn.prepare_cached(&cand_sql)?;
    let mut out = Vec::new();
    for (wid, dist) in windows {
        if dist > cutoff {
            break;
        }
        let mut p: Vec<Value> = vec![Value::Integer(wid)];
        p.extend(where_params.iter().cloned());
        let msgs: Vec<(i64, Option<String>)> = cand
            .query_map(params_from_iter(p.iter()), |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        if msgs.is_empty() {
            continue;
        }
        // Prefer a keyword hit inside the window, then word overlap, then length.
        let anchor = msgs
            .iter()
            .filter_map(|(id, _)| keyword_rank.get(id).map(|r| (*r, *id)))
            .min()
            .map(|(_, id)| id)
            .unwrap_or_else(|| {
                msgs.iter()
                    .max_by_key(|(_, t)| {
                        let t = t.as_deref().unwrap_or_default().to_lowercase();
                        let overlap = words.iter().filter(|w| t.contains(w.as_str())).count();
                        (overlap, t.len().min(280))
                    })
                    .map(|(id, _)| *id)
                    .expect("msgs is non-empty")
            });
        if !out.contains(&anchor) {
            out.push(anchor);
        }
    }
    Ok(out)
}

/// Resolve `from:` and `in:` to ids. None when a value matches nothing.
fn resolve_filters(conn: &Connection, q: &ParsedQuery) -> Result<Option<Filters>, Error> {
    let mut from = FromFilter::default();
    for v in &q.from {
        if v.eq_ignore_ascii_case("me") {
            from.me = true;
            continue;
        }
        let ids = match_handles(conn, v)?;
        if ids.is_empty() {
            return Ok(None);
        }
        from.handles.extend(ids);
    }
    let chats = if q.chat.is_empty() {
        None
    } else {
        let mut all: Option<Vec<i64>> = None;
        // Several in:/with: values narrow down (a chat with Sarah AND Mike).
        for v in &q.chat {
            let ids = match_chats(conn, v)?;
            all = Some(match all {
                None => ids,
                Some(prev) => prev.into_iter().filter(|c| ids.contains(c)).collect(),
            });
        }
        let all = all.unwrap_or_default();
        if all.is_empty() {
            return Ok(None);
        }
        Some(all)
    };
    Ok(Some((from, chats)))
}

fn match_handles(conn: &Connection, v: &str) -> Result<Vec<i64>, Error> {
    let digits: String = v.chars().filter(char::is_ascii_digit).collect();
    let like = format!("%{}%", v.trim());
    let mut ids: Vec<i64> = conn
        .prepare_cached("SELECT id FROM handles WHERE name LIKE ?1 OR address LIKE ?1")?
        .query_map([&like], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if digits.len() >= 4 {
        let d = format!("%{digits}%");
        let more: Vec<i64> = conn
            .prepare_cached("SELECT id FROM handles WHERE digits LIKE ?1")?
            .query_map([&d], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        ids.extend(more);
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn match_chats(conn: &Connection, v: &str) -> Result<Vec<i64>, Error> {
    let like = format!("%{}%", v.trim());
    let mut ids: Vec<i64> = conn
        .prepare_cached("SELECT id FROM chats WHERE title LIKE ?1 OR display_name LIKE ?1")?
        .query_map([&like], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let handles = match_handles(conn, v)?;
    if !handles.is_empty() {
        let list = handles
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let more: Vec<i64> = conn
            .prepare(&format!(
                "SELECT DISTINCT chat_id FROM chat_handles WHERE handle_id IN ({list})"
            ))?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        ids.extend(more);
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// SQL (starting with " AND ...") over alias `m` for every non-text filter.
fn filter_sql(
    q: &ParsedQuery,
    (from, chats): &(FromFilter, Option<Vec<i64>>),
) -> (String, Vec<Value>) {
    let mut sql = String::new();
    let mut p: Vec<Value> = Vec::new();
    // Numbered placeholders continue after the caller's own (?1 = MATCH / window id).
    let next = |p: &mut Vec<Value>, v: Value| -> String {
        p.push(v);
        format!("?{}", p.len() + 1)
    };
    if from.me || !from.handles.is_empty() {
        let mut ors = Vec::new();
        if from.me {
            ors.push("m.from_me = 1".to_string());
        }
        if !from.handles.is_empty() {
            let list = from
                .handles
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            ors.push(format!("(m.from_me = 0 AND m.handle_id IN ({list}))"));
        }
        sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
    }
    if let Some(chats) = chats {
        let list = chats
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(",");
        sql.push_str(&format!(" AND m.chat_id IN ({list})"));
    }
    if let Some(a) = q.after_ms {
        let ph = next(&mut p, Value::Integer(a));
        sql.push_str(&format!(" AND m.date_ms >= {ph}"));
    }
    if let Some(b) = q.before_ms {
        let ph = next(&mut p, Value::Integer(b));
        sql.push_str(&format!(" AND m.date_ms < {ph}"));
    }
    for h in &q.has {
        let cond = match h {
            HasFilter::Link => "m.has_link = 1".to_string(),
            HasFilter::Attachment => "m.attach_count > 0".to_string(),

            HasFilter::Photo => kind_exists(&[AttachmentKind::Image]),
            HasFilter::Video => kind_exists(&[AttachmentKind::Video]),
            HasFilter::Audio => kind_exists(&[AttachmentKind::Audio]),
            HasFilter::File => kind_exists(&[AttachmentKind::File]),
        };
        sql.push_str(&format!(" AND {cond}"));
    }
    (sql, p)
}

/// Driven from the attachments index, so a rare kind (few videos) doesn't
/// scan every message.
fn kind_exists(kinds: &[AttachmentKind]) -> String {
    let list = kinds
        .iter()
        .map(|k| k.code().to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!("m.id IN (SELECT a.message_id FROM attachments a WHERE a.kind IN ({list}))")
}

fn build_hits(
    conn: &Connection,
    ranked: Vec<Ranked>,
    q: &ParsedQuery,
    fts: Option<&str>,
    default: MatchedBy,
) -> Result<Vec<SearchHit>, Error> {
    let _ = fts;
    let mut stmt = conn.prepare_cached(
        "SELECT m.chat_id, COALESCE(c.title, ''), COALESCE(c.is_group, 0), m.from_me,
                COALESCE(h.name, h.address), m.date_ms, m.text, m.attach_count
         FROM messages m
         LEFT JOIN chats c ON c.id = m.chat_id
         LEFT JOIN handles h ON h.id = m.handle_id
         WHERE m.id = ?1",
    )?;
    // Highlight the tokens FTS matched (so "it's" marks "it" and "s"), but
    // not filler words of a question ("where should we eat") when it has
    // meaningful ones: bolding every "we" and "in" makes snippets noisy.
    let tokens = |w: &String| -> Vec<String> {
        w.split(|c: char| !c.is_alphanumeric())
            .map(str::to_lowercase)
            .filter(|t| !t.is_empty())
            .collect()
    };
    let word_tokens: Vec<String> = q.words.iter().flat_map(tokens).collect();
    let has_content = word_tokens.iter().any(|t| !query::is_stopword(t));
    let words: Vec<String> = word_tokens
        .into_iter()
        .filter(|t| !has_content || !query::is_stopword(t))
        .chain(q.phrases.iter().flat_map(tokens))
        .collect();
    let mut people_of: HashMap<i64, Vec<Person>> = HashMap::new();
    let mut out = Vec::with_capacity(ranked.len());
    for r in ranked {
        let Some(row) = stmt
            .query_row([r.id], |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, bool>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            })
            .optional()?
        else {
            continue;
        };
        let (chat_id, title, is_group, from_me, sender, date_ms, text, attach) = row;
        let matched_by = match (r.keyword, r.meaning) {
            (true, true) => MatchedBy::Both,
            (false, true) => MatchedBy::Meaning,
            (true, false) => MatchedBy::Keyword,
            (false, false) => default,
        };
        let t = text
            .as_deref()
            .unwrap_or(if attach > 0 { "Attachment" } else { "" });
        let snippet = snippet(t, &words, SNIPPET_CHARS);
        out.push(SearchHit {
            message_id: r.id,
            chat_id,
            chat_title: title,
            is_group,
            from_me,
            sender: if from_me { None } else { sender },
            date_ms,
            snippet,
            matched_by,
            attachment_count: attach,
            score: r.score,
            people: match chat_id {
                Some(c) => match people_of.get(&c) {
                    Some(p) => p.clone(),
                    None => {
                        let mut p = crate::store::participants(conn, c)?;
                        p.truncate(4);
                        people_of.insert(c, p.clone());
                        p
                    }
                },
                None => Vec::new(),
            },
        });
    }
    Ok(out)
}

/// Characters of message text shown per result.
const SNIPPET_CHARS: usize = 180;

/// A window of `text` around the first match, with every match wrapped in
/// U+0002/U+0003. Matches start at word boundaries, like FTS prefix terms.
/// (Computed here rather than with FTS5 snippet(): one pass over ~50 short
/// strings instead of one MATCH query per result.)
pub(crate) fn snippet(text: &str, words: &[String], max_chars: usize) -> String {
    let lower = text.to_lowercase();
    // Lowercasing can change byte lengths in a few scripts; then skip marks.
    let marks = if lower.len() == text.len() {
        match_marks(text, &lower, words)
    } else {
        vec![false; text.len()]
    };
    let first = marks.iter().position(|m| *m).unwrap_or(0);

    // Start ~40 chars before the first match, at a word boundary.
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let first_char = chars.iter().position(|(b, _)| *b >= first).unwrap_or(0);
    let mut start = if chars.len() <= max_chars {
        0
    } else {
        first_char.saturating_sub(40)
    };
    while start > 0 && start < first_char && !chars[start - 1].1.is_whitespace() {
        start += 1;
    }
    let end = (start + max_chars).min(chars.len());

    let mut out = String::with_capacity(max_chars * 4 + 8);
    if start > 0 {
        out.push('…');
    }
    let mut on = false;
    for &(b, ch) in &chars[start..end] {
        if marks[b] != on {
            out.push(if marks[b] { '\u{2}' } else { '\u{3}' });
            on = marks[b];
        }
        out.push(ch);
    }
    if on {
        out.push('\u{3}');
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// Byte mask of case-insensitive, word-start matches of `words`.
fn match_marks(text: &str, lower: &str, words: &[String]) -> Vec<bool> {
    let mut marks = vec![false; text.len()];
    for w in words {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(w.as_str()) {
            let s = from + pos;
            let at_word_start = lower[..s]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric());
            if at_word_start {
                for m in &mut marks[s..s + w.len()] {
                    *m = true;
                }
            }
            from = s + w.len().max(1);
        }
    }
    marks
}
