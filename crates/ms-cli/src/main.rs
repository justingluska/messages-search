//! `ms`: index and search Messages from the terminal (dev + benchmarks).
//!
//!   ms index  [--db chat.db] [--index index.db] [--full]
//!   ms search [--index index.db] <query...>
//!   ms stats  [--index index.db]
//!   ms bench  [--index index.db] [--runs 50]
//!   ms eval   [--index index.db] --needles fixtures/chat.needles.json
//!
//! `--models DIR` enables meaning search (downloads the model on first use);
//! `--names FILE` sets contact names from a fixture's needles JSON.

use std::path::PathBuf;
use std::time::Instant;

use ms_core::Store;
use ms_source::{sync, Source};

fn usage() -> ! {
    eprintln!("usage: ms <index|search|stats|bench> [--db PATH] [--index PATH] [--full] [--runs N] [query...]");
    std::process::exit(2)
}

struct Args {
    cmd: String,
    db: PathBuf,
    index: PathBuf,
    full: bool,
    runs: usize,
    models: Option<PathBuf>,
    names: Option<PathBuf>,
    needles: Option<PathBuf>,
    rest: Vec<String>,
}

fn parse_args() -> Args {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_else(|| usage());
    let mut a = Args {
        cmd,
        db: ms_source::default_db_path(),
        index: PathBuf::new(),
        full: false,
        runs: 50,
        models: None,
        names: None,
        needles: None,
        rest: Vec::new(),
    };
    let mut have_index = false;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--db" => a.db = it.next().map(PathBuf::from).unwrap_or_else(|| usage()),
            "--index" => {
                a.index = it.next().map(PathBuf::from).unwrap_or_else(|| usage());
                have_index = true;
            }
            "--full" => a.full = true,
            "--runs" => {
                a.runs = it
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--models" => a.models = it.next().map(PathBuf::from),
            "--names" => a.names = it.next().map(PathBuf::from),
            "--needles" => a.needles = it.next().map(PathBuf::from),
            _ => a.rest.push(arg),
        }
    }
    // Required on purpose: an index built from a real chat.db holds every
    // message, so it must never land in the working directory (the repo).
    if !have_index {
        eprintln!("--index PATH is required (keep it outside the repo)");
        std::process::exit(2);
    }
    a
}

fn load_embedder(models: &Option<PathBuf>) -> Option<ms_embed::FastEmbedder> {
    let dir = models.as_ref()?;
    let t = Instant::now();
    let e = ms_embed::FastEmbedder::new(ms_embed::ModelChoice::default(), dir, None)
        .unwrap_or_else(|e| {
            eprintln!("model: {e}");
            std::process::exit(1)
        });
    eprintln!("model loaded in {:.1}s", t.elapsed().as_secs_f64());
    Some(e)
}

/// Contact names from a fixture's needles JSON (`contacts`: address → name).
fn apply_fixture_names(path: &PathBuf, source: &Source, store: &Store) {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read names"))
            .expect("names json");
    let names: std::collections::HashMap<String, String> = json["contacts"]
        .as_object()
        .expect("contacts object")
        .iter()
        .filter_map(|(addr, name)| {
            Some((
                ms_source::contacts::key_for(addr)?,
                name.as_str()?.to_string(),
            ))
        })
        .collect();
    let resolved = ms_source::contacts::resolve(&source.handles().expect("handles"), &names);
    let chats = store.set_handle_names(&resolved).expect("set names");
    for c in &chats {
        store.rebuild_windows(*c, i64::MIN / 2).expect("rebuild");
    }
    println!("names: {} chats renamed", chats.len());
}

fn main() {
    let a = parse_args();
    let store = Store::open(&a.index, ms_core::Tz::System).unwrap_or_else(|e| {
        eprintln!("can't open index {}: {e}", a.index.display());
        std::process::exit(1)
    });
    match a.cmd.as_str() {
        "index" => {
            let source = Source::open(&a.db).unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(1)
            });
            let t = Instant::now();
            let report = sync::sync(
                &source,
                &store,
                a.full || store.meta("last_date_ms").ok().flatten().is_none(),
                a.names.is_none(),
                &|p| {
                    if p.total > 0 && p.done % 20_000 < 2000 {
                        eprintln!("  {:?} {}/{}", p.phase, p.done, p.total);
                    }
                },
            )
            .unwrap_or_else(|e| {
                eprintln!("sync failed: {e}");
                std::process::exit(1)
            });
            println!("{report:?} in {:.2}s", t.elapsed().as_secs_f64());
            if let Some(names) = &a.names {
                apply_fixture_names(names, &source, &store);
            }
            if let Some(e) = load_embedder(&a.models) {
                let t = Instant::now();
                let stop = std::sync::atomic::AtomicBool::new(false);
                let n = ms_core::embed::embed_pending(
                    &store,
                    &e,
                    &|p| {
                        if p.done % 2048 < 32 {
                            eprintln!("  embedding {}/{}", p.done, p.total);
                        }
                    },
                    &stop,
                )
                .expect("embed");
                let secs = t.elapsed().as_secs_f64();
                println!(
                    "embedded {n} windows in {secs:.1}s ({:.0}/s)",
                    n as f64 / secs.max(0.001)
                );
            }
            print_stats(&store);
        }
        "search" => {
            let q = a.rest.join(" ");
            let e = load_embedder(&a.models);
            let r = store
                .search(&q, 20, e.as_ref().map(|e| e as &dyn ms_core::Embedder))
                .unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(1)
                });
            println!(
                "{} hits in {:.2} ms (semantic: {})",
                r.hits.len(),
                r.took_ms,
                r.semantic
            );
            for h in &r.hits {
                let who = if h.from_me {
                    "Me".to_string()
                } else {
                    h.sender.clone().unwrap_or_default()
                };
                let snip = h.snippet.replace('\u{2}', "[").replace('\u{3}', "]");
                println!(
                    "  #{:<7} {:<22} {:<16} {:?}  {}",
                    h.message_id,
                    trunc(&h.chat_title, 22),
                    trunc(&who, 16),
                    h.matched_by,
                    trunc(&snip, 90)
                );
            }
        }
        "stats" => print_stats(&store),
        "bench" => bench(&store, a.runs),
        "insights" => {
            for year in [None, Some(2024)] {
                let t = Instant::now();
                let i = store.insights(year, i64::MAX / 4).expect("insights");
                println!(
                    "{year:?}: {} msgs, {} days, top person {:?}, longest streak {} in {:.1} ms",
                    i.total_messages,
                    i.days.len(),
                    i.top_people.first().map(|p| p.total),
                    i.longest_streak,
                    t.elapsed().as_secs_f64() * 1000.0
                );
            }
            let t = Instant::now();
            let s = store.storage_summary().expect("storage");
            println!(
                "storage: {} files, {:.1} MB in {:.1} ms",
                s.total_count,
                s.total_bytes as f64 / 1e6,
                t.elapsed().as_secs_f64() * 1000.0
            );
        }
        "eval" => {
            let needles = a.needles.clone().unwrap_or_else(|| usage());
            let e = load_embedder(&a.models);
            eval(
                &store,
                &needles,
                e.as_ref().map(|e| e as &dyn ms_core::Embedder),
            );
        }
        "synth" => {
            let n: usize = a
                .rest
                .first()
                .and_then(|n| n.parse().ok())
                .unwrap_or(500_000);
            let t = Instant::now();
            synth(&store, n);
            println!("synth {n} messages in {:.1}s", t.elapsed().as_secs_f64());
            print_stats(&store);
        }
        _ => usage(),
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n - 1).collect::<String>() + "…"
    }
}

fn print_stats(store: &Store) {
    match store.stats() {
        Ok(s) => {
            println!(
            "messages {} · chats {} · attachments {} · windows {} (embedded {}) · index {:.1} MB",
            s.messages, s.chats, s.attachments, s.windows, s.embedded_windows,
            s.index_bytes as f64 / 1_048_576.0
        )
        }
        Err(e) => eprintln!("{e}"),
    }
}

/// Fill the index with `n` synthetic chat messages (Zipf-ish vocabulary,
/// bursts per chat) to measure search at real-history scale. Dev only.
fn synth(store: &Store, n: usize) {
    use ms_core::types::*;
    const WORDS: &[&str] = &[
        "the",
        "you",
        "i",
        "to",
        "a",
        "and",
        "it",
        "is",
        "that",
        "lol",
        "ok",
        "yeah",
        "me",
        "on",
        "in",
        "for",
        "what",
        "so",
        "are",
        "just",
        "at",
        "do",
        "we",
        "haha",
        "can",
        "be",
        "my",
        "no",
        "get",
        "like",
        "now",
        "go",
        "time",
        "tomorrow",
        "tonight",
        "dinner",
        "home",
        "work",
        "love",
        "good",
        "see",
        "call",
        "later",
        "thanks",
        "omw",
        "here",
        "there",
        "going",
        "want",
        "need",
        "know",
        "think",
        "really",
        "today",
        "night",
        "morning",
        "weekend",
        "friday",
        "saturday",
        "sunday",
        "monday",
        "party",
        "birthday",
        "flight",
        "airport",
        "hotel",
        "wedding",
        "restaurant",
        "tacos",
        "pizza",
        "coffee",
        "beach",
        "house",
        "car",
        "game",
        "movie",
        "gym",
        "doctor",
        "appointment",
        "meeting",
        "address",
        "code",
        "gate",
        "password",
        "link",
        "photo",
        "picture",
        "video",
        "money",
        "venmo",
        "rent",
        "trip",
        "vacation",
        "miami",
        "austin",
        "new",
        "york",
        "boston",
        "mom",
        "dad",
        "kids",
        "dog",
        "school",
        "class",
        "exam",
        "project",
        "client",
        "deadline",
        "invoice",
        "contract",
        "lunch",
        "breakfast",
        "drinks",
        "bar",
        "concert",
        "tickets",
        "uber",
        "traffic",
    ];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let chats = 400i64;
    let handles: Vec<IngestHandle> = (1..=300)
        .map(|i| IngestHandle {
            id: i,
            address: format!("+1555555{i:04}"),
            name: Some(format!("Person {i}")),
        })
        .collect();
    store.upsert_handles(&handles).unwrap();
    let chat_rows: Vec<IngestChat> = (1..=chats)
        .map(|c| IngestChat {
            id: c,
            identifier: format!("chat{c}"),
            display_name: None,
            service: Some("iMessage".into()),
            participants: if c % 10 == 0 {
                vec![c % 300 + 1, (c * 7) % 300 + 1, (c * 13) % 300 + 1]
            } else {
                vec![c % 300 + 1]
            },
        })
        .collect();
    store.upsert_chats(&chat_rows).unwrap();
    let start = 1_546_300_800_000i64; // 2019-01-01
    let span = 7 * 365 * 86_400_000i64;
    let mut batch = Vec::with_capacity(5000);
    let mut touched = std::collections::BTreeMap::new();
    let mut date = start;
    let mut chat = 1i64;
    let mut burst_left = 0u64;
    // Average burst is ~15 messages; spread bursts over the whole span.
    let burst_gap = (span as u64 / (n as u64 / 15).max(1)).max(1);
    for id in 1..=n as i64 {
        // Real conversations are bursts: a chat exchanges a run of messages
        // seconds apart, then goes quiet.
        if burst_left == 0 {
            burst_left = 1 + rnd() % 30;
            chat = 1 + ((rnd() % 1000) as f64).powf(1.6) as i64 % chats;
            date += (rnd() % (burst_gap * 2)) as i64;
        } else {
            date += (rnd() % 90_000) as i64;
        }
        burst_left -= 1;
        let len = 1 + (rnd() % 12) as usize;
        let text: Vec<&str> = (0..len)
            .map(|_| {
                // Zipf-ish: small indices much more likely.
                let r = (rnd() % 10_000) as f64 / 10_000.0;
                WORDS[((r * r * r) * WORDS.len() as f64) as usize % WORDS.len()]
            })
            .collect();
        let from_me = rnd() % 2 == 0;
        batch.push(IngestMessage {
            id,
            guid: format!("S{id}"),
            chat_id: Some(chat),
            handle_id: if from_me { None } else { Some(chat % 300 + 1) },
            from_me,
            date_ms: date,
            text: Some(text.join(" ")),
            kind: MessageKind::Text,
            service: Some("iMessage".into()),
            reply_to_guid: None,
            edited: false,
            unsent: false,
            attachments: if rnd() % 25 == 0 {
                vec![IngestAttachment {
                    id,
                    filename: Some(format!("IMG_{id}.HEIC")),
                    mime: Some("image/heic".into()),
                    path: None,
                    bytes: 2_000_000,
                    kind: AttachmentKind::Image,
                }]
            } else {
                vec![]
            },
            reaction: None,
        });
        if batch.len() == 5000 {
            for (c, f) in store.ingest_messages(&batch).unwrap() {
                let e = touched.entry(c).or_insert(f);
                *e = (*e).min(f);
            }
            batch.clear();
        }
    }
    for (c, f) in store.ingest_messages(&batch).unwrap() {
        let e = touched.entry(c).or_insert(f);
        *e = (*e).min(f);
    }
    let t = Instant::now();
    for (c, f) in touched {
        store.rebuild_windows(c, f).unwrap();
    }
    println!("windows built in {:.1}s", t.elapsed().as_secs_f64());
}

/// For each planted fact's paraphrased queries: the rank of the first result
/// inside the fact's conversation, keyword-only vs hybrid.
fn eval(store: &Store, needles: &PathBuf, embedder: Option<&dyn ms_core::Embedder>) {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(needles).expect("read")).expect("json");
    let rank =
        |q: &str, e: Option<&dyn ms_core::Embedder>, guids: &[String]| -> (Option<usize>, f64) {
            let r = store.search(q, 20, e).expect("search");
            let pos = r.hits.iter().position(|h| {
                store
                    .messages_around(h.message_id, 0, 0)
                    .ok()
                    .and_then(|m| m.into_iter().next())
                    .is_some_and(|m| guids.contains(&m.guid))
            });
            (pos, r.took_ms)
        };
    let fmt = |p: Option<usize>| p.map_or("-".to_string(), |p| (p + 1).to_string());
    let (mut n, mut kw1, mut kw5, mut hy1, mut hy5) = (0, 0, 0, 0, 0);
    let mut times = Vec::new();
    println!("{:<48} {:>8} {:>8}", "query", "keyword", "hybrid");
    for needle in json["needles"].as_array().expect("needles") {
        let guids: Vec<String> = needle["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .filter_map(|m| m["guid"].as_str().map(str::to_string))
            .collect();
        for q in needle["queries"].as_array().expect("queries") {
            let q = q.as_str().expect("query");
            let (k, _) = rank(q, None, &guids);
            let (h, ms) = match embedder {
                Some(e) => rank(q, Some(e), &guids),
                None => (None, 0.0),
            };
            times.push(ms);
            n += 1;
            kw1 += usize::from(k == Some(0));
            kw5 += usize::from(k.is_some_and(|p| p < 5));
            hy1 += usize::from(h == Some(0));
            hy5 += usize::from(h.is_some_and(|p| p < 5));
            println!("{:<48} {:>8} {:>8}", trunc(q, 48), fmt(k), fmt(h));
        }
    }
    println!(
        "\n{n} queries · keyword: top-1 {kw1}, top-5 {kw5} · hybrid: top-1 {hy1}, top-5 {hy5}"
    );
    if embedder.is_some() {
        times.sort_by(f64::total_cmp);
        println!(
            "hybrid latency (incl. query embedding): p50 {:.1} ms, max {:.1} ms",
            times[times.len() / 2],
            times[times.len() - 1]
        );
    }
}

/// Latency of typical queries, including as-you-type prefixes.
fn bench(store: &Store, runs: usize) {
    let queries = [
        "g",
        "ga",
        "gat",
        "gate",
        "gate code",
        "the",
        "you",
        "dinner",
        "flight",
        "https",
        "from:me",
        "has:photo",
        "has:link",
        "during:2024",
        "from:me has:link",
        "\"see you\"",
        "love you",
        "tomorrow night",
        "address",
        "birthday party",
    ];
    println!(
        "{:<22} {:>8} {:>8} {:>8} {:>6}",
        "query", "p50 ms", "p95 ms", "max ms", "hits"
    );
    let mut all = Vec::new();
    for q in queries {
        let mut times = Vec::with_capacity(runs);
        let mut hits = 0;
        for _ in 0..runs {
            let t = Instant::now();
            let r = store.search(q, 50, None).expect("search");
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            hits = r.hits.len();
        }
        times.sort_by(f64::total_cmp);
        let p = |x: f64| times[((times.len() as f64 - 1.0) * x) as usize];
        println!(
            "{:<22} {:>8.2} {:>8.2} {:>8.2} {:>6}",
            q,
            p(0.5),
            p(0.95),
            times[times.len() - 1],
            hits
        );
        all.extend(times);
    }
    all.sort_by(f64::total_cmp);
    println!(
        "overall p50 {:.2} ms, p95 {:.2} ms",
        all[all.len() / 2],
        all[(all.len() as f64 * 0.95) as usize]
    );
}
