//! Activity stats (the Insights page): one pass over the messages in range,
//! bucketed by local day, hour and weekday, plus per-chat totals.

use std::collections::HashMap;

use rusqlite::params;

use crate::store::{participants, Store};
use crate::types::*;
use crate::Error;

const TOP_PEOPLE: usize = 20;
const TOP_GROUPS: usize = 10;

impl Store {
    /// Stats for `year` (local calendar year), or all time when None.
    /// `today_ms` anchors the current streak.
    pub fn insights(&self, year: Option<i32>, today_ms: i64) -> Result<Insights, Error> {
        let tz = self.tz();
        let (from, to) = match year {
            Some(y) => crate::query::date_range(&y.to_string(), tz)
                .ok_or_else(|| Error::Other(format!("bad year {y}")))?,
            None => (i64::MIN, i64::MAX),
        };
        let conn = self.reader();

        let years: Vec<i32> = {
            let (min, max): (Option<i64>, Option<i64>) = conn.query_row(
                "SELECT MIN(date_ms), MAX(date_ms) FROM messages WHERE kind=0",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            match (min, max) {
                (Some(a), Some(b)) => (crate::store::civil_from_days(tz.local_day(a)).0
                    ..=crate::store::civil_from_days(tz.local_day(b)).0)
                    .map(|y| y as i32)
                    .collect(),
                _ => Vec::new(),
            }
        };

        // One pass: ~300k rows is a few tens of ms.
        let mut days: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
        let mut by_hour = vec![0i64; 24];
        let mut by_weekday = vec![0i64; 7];
        let mut per_chat: HashMap<i64, (i64, i64)> = HashMap::new(); // (sent, received)
        let mut offsets: HashMap<i64, i64> = HashMap::new();
        let (mut total, mut sent) = (0i64, 0i64);
        let (mut first, mut last): (Option<i64>, Option<i64>) = (None, None);
        {
            let mut stmt = conn.prepare_cached(
                "SELECT date_ms, from_me, chat_id FROM messages
                 WHERE kind=0 AND date_ms >= ?1 AND date_ms < ?2",
            )?;
            let mut rows = stmt.query(params![from, to])?;
            while let Some(r) = rows.next()? {
                let date: i64 = r.get(0)?;
                let from_me: bool = r.get(1)?;
                let chat: Option<i64> = r.get(2)?;
                // Offset per UTC day (DST changes it; the lookup isn't free).
                let utc_day = date.div_euclid(86_400_000);
                let off = *offsets.entry(utc_day).or_insert_with(|| tz.offset_at(date));
                let local_s = date.div_euclid(1000) + off;
                let day = local_s.div_euclid(86_400);
                *days.entry(day).or_insert(0) += 1;
                by_hour[(local_s.rem_euclid(86_400) / 3600) as usize] += 1;
                // 1970-01-01 was a Thursday (4).
                by_weekday[((day + 4).rem_euclid(7)) as usize] += 1;
                total += 1;
                if from_me {
                    sent += 1;
                }
                if let Some(c) = chat {
                    let e = per_chat.entry(c).or_insert((0, 0));
                    if from_me {
                        e.0 += 1;
                    } else {
                        e.1 += 1;
                    }
                }
                first = Some(first.map_or(date, |f: i64| f.min(date)));
                last = Some(last.map_or(date, |l: i64| l.max(date)));
            }
        }

        let (longest_streak, current_streak) = streaks(&days, tz.local_day(today_ms));
        let busiest_day = days
            .iter()
            .max_by_key(|(d, c)| (**c, -**d))
            .map(|(d, c)| DayCount {
                day: day_string(*d),
                count: *c,
            });

        // Top 1:1 people and groups.
        let mut chats: Vec<(i64, i64, i64)> =
            per_chat.into_iter().map(|(c, (s, r))| (c, s, r)).collect();
        chats.sort_by_key(|(c, s, r)| (-(s + r), *c));
        let mut is_group = conn.prepare_cached("SELECT is_group, title FROM chats WHERE id=?1")?;
        let mut top_people = Vec::new();
        let mut top_groups = Vec::new();
        for (chat, s, r) in &chats {
            if top_people.len() >= TOP_PEOPLE && top_groups.len() >= TOP_GROUPS {
                break;
            }
            let Some((group, title)): Option<(bool, String)> = is_group
                .query_row([chat], |row| Ok((row.get(0)?, row.get(1)?)))
                .ok()
            else {
                continue;
            };
            let people = participants(&conn, *chat)?;
            if group {
                if top_groups.len() < TOP_GROUPS {
                    top_groups.push(GroupStat {
                        chat_id: *chat,
                        title,
                        total: s + r,
                        people: people.into_iter().take(4).collect(),
                    });
                }
            } else if top_people.len() < TOP_PEOPLE {
                let Some(person) = people.into_iter().next() else {
                    continue;
                };
                // The same person can have several 1:1 chats (number + email).
                if let Some(p) = top_people.iter_mut().find(|p: &&mut PersonStat| {
                    p.person.name.is_some() && p.person.name == person.name
                }) {
                    p.sent += s;
                    p.received += r;
                    p.total += s + r;
                    continue;
                }
                top_people.push(PersonStat {
                    person,
                    chat_id: Some(*chat),
                    sent: *s,
                    received: *r,
                    total: s + r,
                });
            }
        }
        top_people.sort_by_key(|p| -p.total);

        let (attachments_count, attachments_bytes): (i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(a.bytes), 0) FROM attachments a JOIN messages m ON m.id = a.message_id
             WHERE m.date_ms >= ?1 AND m.date_ms < ?2",
            params![from, to],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        Ok(Insights {
            year,
            years,
            total_messages: total,
            sent,
            received: total - sent,
            chats: chats.len() as i64,
            first_ms: first,
            last_ms: last,
            days: days
                .into_iter()
                .map(|(d, c)| DayCount {
                    day: day_string(d),
                    count: c,
                })
                .collect(),
            by_hour,
            by_weekday,
            top_people,
            top_groups,
            longest_streak,
            current_streak,
            busiest_day,
            attachments_bytes,
            attachments_count,
        })
    }
}

/// (longest run of consecutive days, run ending today or yesterday).
fn streaks(days: &std::collections::BTreeMap<i64, i64>, today: i64) -> (i64, i64) {
    let (mut longest, mut run, mut prev) = (0i64, 0i64, i64::MIN);
    for d in days.keys() {
        run = if prev != i64::MIN && *d == prev + 1 {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
        prev = *d;
    }
    let current = if prev == today || prev == today - 1 {
        run
    } else {
        0
    };
    (longest, current)
}

fn day_string(day: i64) -> String {
    let (y, m, d) = crate::store::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streak_math() {
        let days: std::collections::BTreeMap<i64, i64> =
            [(10, 1), (11, 2), (12, 1), (20, 5), (21, 1)]
                .into_iter()
                .collect();
        assert_eq!(streaks(&days, 21), (3, 2));
        assert_eq!(streaks(&days, 22), (3, 2));
        assert_eq!(streaks(&days, 30), (3, 0));
        assert_eq!(day_string(0), "1970-01-01");
    }
}
