//! Sanity check + throughput for the embedder, on fictional chat snippets.
//!
//!   cargo run -p ms-embed --release --example similarity -- <cache_dir> [e5]
//!
//! Downloads the model into <cache_dir> on first run.

use std::time::Instant;

use ms_core::types::Embedder;
use ms_embed::{DownloadEvent, FastEmbedder, ModelChoice};

const PASSAGES: &[&str] = &[
    "Maya: the gate code is 4471# btw, just punch it in and drive up to the second house",
    "Leo: we tried that taco place on South Congress in Austin, Veracruz All Natural, honestly the best migas I've had",
    "Priya: wedding is June 14th! we blocked rooms at the Hotel Emma in San Antonio under Patel-Nguyen",
    "me: landing at 6:40pm, flight UA 1423 from Denver",
    "Sam: happy birthday!! 🎂 hope 31 treats you well",
    "Nora: can you grab oat milk and eggs on your way home",
    "Diego: ¿nos vemos el sábado para la cena en casa de mi abuela?",
    "Jonah: the new address is 1180 Alder Street, Apt 3B, Portland OR",
    "Ava: my car battery died again, stuck in the parking lot at work",
    "Maya: dentist moved my cleaning to Tuesday at 9",
];

const QUERIES: &[(&str, usize)] = &[
    ("what's the code to get in the gate", 0),
    ("restaurant recommendation in Austin", 1),
    ("when and where is the wedding", 2),
    ("what flight is arriving tonight", 3),
    ("someone's birthday", 4),
    ("grocery list", 5),
    ("dinner at grandma's this weekend", 6),
    ("where did Jonah move", 7),
    ("car trouble", 8),
    ("doctor appointment", 9),
];

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cache_dir = args.next().expect("usage: similarity <cache_dir> [e5]");
    let choice = match args.next().as_deref() {
        Some("e5") => ModelChoice::MultilingualE5Small,
        _ => ModelChoice::BgeSmallEnV15Q,
    };

    let t = Instant::now();
    let embedder = FastEmbedder::new(
        choice,
        std::path::Path::new(&cache_dir),
        Some(Box::new(|e: DownloadEvent| match e {
            DownloadEvent::Progress {
                done_bytes,
                total_bytes,
            } => {
                if done_bytes == total_bytes || done_bytes % (8 << 20) < (256 << 10) {
                    eprintln!(
                        "  download {:.1}/{:.1} MB",
                        done_bytes as f64 / 1e6,
                        total_bytes as f64 / 1e6
                    )
                }
            }
            other => eprintln!("  {other:?}"),
        })),
    )
    .expect("load model");
    println!(
        "model {} ({} dims) loaded in {:?}",
        embedder.model_id(),
        embedder.dims(),
        t.elapsed()
    );

    let passages: Vec<String> = PASSAGES.iter().map(|s| s.to_string()).collect();
    let pv = embedder.embed_passages(&passages).expect("embed passages");
    for v in &pv {
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "not normalized: {norm}");
    }

    let mut correct = 0;
    for (q, expected) in QUERIES {
        let qv = embedder.embed_query(q).expect("embed query");
        let mut scored: Vec<(usize, f32)> = pv.iter().map(|p| cosine(&qv, p)).enumerate().collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let (best, best_s) = scored[0];
        let exp_s = cosine(&qv, &pv[*expected]);
        let ok = best == *expected;
        correct += ok as usize;
        println!(
            "{} {:<38} expected {:.3} | top #{best} {:.3} (2nd {:.3})",
            if ok { "ok  " } else { "MISS" },
            q,
            exp_s,
            best_s,
            scored[1].1
        );
    }
    println!("top-1: {correct}/{}", QUERIES.len());

    // Throughput: realistic window-sized passages (~150-900 chars), batches of 64.
    let windows: Vec<String> = (0..512)
        .map(|i| {
            let n = 1 + (i * 7) % 12;
            (0..n)
                .map(|j| PASSAGES[(i + j) % PASSAGES.len()])
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();
    let avg_chars = windows.iter().map(String::len).sum::<usize>() / windows.len();
    embedder.embed_passages(&windows[..64]).unwrap(); // warm-up
    let t = Instant::now();
    for chunk in windows.chunks(64) {
        embedder.embed_passages(chunk).unwrap();
    }
    let el = t.elapsed();
    println!(
        "throughput: {} windows (avg {avg_chars} chars) in {:?} = {:.2} ms/window, {:.0} windows/s",
        windows.len(),
        el,
        el.as_secs_f64() * 1000.0 / windows.len() as f64,
        windows.len() as f64 / el.as_secs_f64()
    );
    let t = Instant::now();
    for _ in 0..20 {
        embedder
            .embed_query("what's the code to get in the gate")
            .unwrap();
    }
    println!(
        "query latency: {:.2} ms",
        t.elapsed().as_secs_f64() * 1000.0 / 20.0
    );
}
