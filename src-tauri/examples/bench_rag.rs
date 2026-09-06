//! `status.md` 7.1's real benchmark — measures, on this machine, the three
//! researched retrieval approaches against the real product code (not a
//! throwaway reimplementation): (a) `rag::embed_tfidf` (pure Rust), (b)
//! `rag::embed_onnx` (real MiniLM ONNX inference, loaded from a real
//! downloaded copy of the model — path given via `MINILM_MODEL_DIR`), (c)
//! `rag::embed_cloud` is reasoned about separately (no real API key
//! available this session — see `docs/evidence.md`).
//!
//! Run with:
//! ```text
//! MINILM_MODEL_DIR=/path/to/dir/with/model.onnx+tokenizer.json \
//!   cargo run --release --example bench_rag
//! ```
//! Real numbers from an actual run are recorded in `docs/evidence.md`, not
//! estimated.

use std::time::Instant;

use aside_lib::rag::chunker::chunk_text;
use aside_lib::rag::embed::Embedder;
use aside_lib::rag::embed_onnx::OnnxEmbedder;
use aside_lib::rag::embed_tfidf::HashedTermFrequencyEmbedder;
use aside_lib::rag::retriever::top_k;

/// Builds ~90 short segments representative of what
/// `storage::transcripts::TranscriptSegment` rows look like in a real
/// interview/meeting session — the same rough shape `status.md` 7.1 asks
/// for ("a representative transcript, ~50-100 short segments").
fn synthetic_transcript_segments() -> Vec<String> {
    let topics: &[(&str, &[&str])] = &[
        (
            "background",
            &[
                "Tell me a bit about your background and what you've been working on recently.",
                "I've spent the last four years as a backend engineer focused on distributed systems.",
                "Before that I was at a smaller startup building the initial version of their API.",
                "I mostly work in Rust and Go these days, with some Python for data tooling.",
            ],
        ),
        (
            "visa",
            &[
                "What is your current visa status, and would you need sponsorship?",
                "I am currently on an H1-B visa sponsored by my present employer.",
                "It would need to be transferred if I moved to a new company.",
                "The transfer process usually takes a few weeks once an offer is signed.",
            ],
        ),
        (
            "notice",
            &[
                "How much notice would you need to give your current employer?",
                "My notice period is two weeks, standard for my current contract.",
                "I could potentially start sooner if we worked something out with my manager.",
                "I'd want to leave things in good shape for my team before I go.",
            ],
        ),
        (
            "system_design",
            &[
                "Let's talk about system design — how would you scale a chat application?",
                "I'd start by separating the read and write paths with a message queue.",
                "WebSocket connections would be handled by a stateless fleet behind a load balancer.",
                "For persistence I'd reach for a log-structured store tuned for high write throughput.",
            ],
        ),
        (
            "location",
            &[
                "Are you open to relocating, or do you need to stay remote?",
                "I'm currently based in Austin and would prefer to stay remote if possible.",
                "I'm open to occasional travel for team offsites a few times a year.",
                "My family is settled here so a permanent relocation isn't something I'm considering.",
            ],
        ),
        (
            "coding",
            &[
                "Walk me through how you'd debug a memory leak in a long-running service.",
                "I'd start with a heap profiler to see what's actually accumulating over time.",
                "Then I'd bisect recent deploys to correlate the leak with a specific change.",
                "Once found, I'd add a regression test that fails if the leak reappears.",
            ],
        ),
        (
            "title",
            &[
                "What's your current title, and what would you expect here?",
                "My current title is Senior Software Engineer, and I'd expect something comparable.",
                "I'm less focused on title than on the scope of ownership for the role.",
                "Growth into a tech lead position over time would be ideal for me.",
            ],
        ),
    ];

    let mut segments = Vec::new();
    // Repeat the topic pool with light variation until we have ~90 segments
    // — a real 20-30 minute interview easily produces this many finals.
    let mut round = 0;
    while segments.len() < 90 {
        for (_, lines) in topics {
            for line in *lines {
                if round == 0 {
                    segments.push(line.to_string());
                } else {
                    segments.push(format!("{line} (round {round})"));
                }
                if segments.len() >= 90 {
                    break;
                }
            }
            if segments.len() >= 90 {
                break;
            }
        }
        round += 1;
    }
    segments
}

fn human_ms(d: std::time::Duration) -> String {
    format!("{:.3}ms", d.as_secs_f64() * 1000.0)
}

fn bench_tfidf(segments: &[String], query: &str) {
    println!("\n=== (a) TF-IDF-family: rag::embed_tfidf::HashedTermFrequencyEmbedder ===");
    let embedder = HashedTermFrequencyEmbedder;

    // Indexing: chunk every segment's concatenated text (representative of
    // a real session transcript) and embed each ~80-word chunk.
    let full_text = segments.join(" ");
    let chunks = chunk_text(&full_text);
    println!(
        "chunked {} segments into {} chunks (~80 words each)",
        segments.len(),
        chunks.len()
    );

    let start = Instant::now();
    let mut vectors = Vec::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for chunk in &chunks {
        let v = rt.block_on(embedder.embed(&chunk.text)).unwrap();
        vectors.push(v);
    }
    let index_elapsed = start.elapsed();
    println!(
        "indexing (all {} chunks): {} total, {} per chunk",
        chunks.len(),
        human_ms(index_elapsed),
        human_ms(index_elapsed / chunks.len() as u32)
    );

    // Query: embed the query + rank against every stored vector.
    let candidates: Vec<(uuid::Uuid, Vec<f32>)> = chunks
        .iter()
        .map(|_| uuid::Uuid::new_v4())
        .zip(vectors.iter().cloned())
        .collect();
    let start = Instant::now();
    let query_vec = rt.block_on(embedder.embed(query)).unwrap();
    let results = top_k(&query_vec, &candidates, 5, 0.0);
    let query_elapsed = start.elapsed();
    println!(
        "query (embed + top-5 over {} chunks): {}",
        candidates.len(),
        human_ms(query_elapsed)
    );
    println!(
        "top match score: {:.4}",
        results.first().map(|r| r.score).unwrap_or(0.0)
    );
}

fn bench_onnx(segments: &[String], query: &str) {
    let model_dir = match std::env::var("MINILM_MODEL_DIR") {
        Ok(v) => v,
        Err(_) => {
            println!("\n=== (b) Local ONNX MiniLM: SKIPPED — set MINILM_MODEL_DIR to run ===");
            return;
        }
    };
    println!("\n=== (b) Local ONNX MiniLM: rag::embed_onnx::OnnxEmbedder ===");
    println!("model dir: {model_dir}");

    let load_start = Instant::now();
    let embedder = OnnxEmbedder::load(std::path::Path::new(&model_dir)).expect("load MiniLM model");
    let load_elapsed = load_start.elapsed();
    println!(
        "model load (one-time, at app start): {}",
        human_ms(load_elapsed)
    );

    let full_text = segments.join(" ");
    let chunks = chunk_text(&full_text);
    println!(
        "chunked {} segments into {} chunks (~80 words each)",
        segments.len(),
        chunks.len()
    );

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let start = Instant::now();
    let mut vectors = Vec::new();
    for chunk in &chunks {
        let v = rt.block_on(embedder.embed(&chunk.text)).unwrap();
        vectors.push(v);
    }
    let index_elapsed = start.elapsed();
    println!(
        "indexing (all {} chunks): {} total, {} per chunk",
        chunks.len(),
        human_ms(index_elapsed),
        human_ms(index_elapsed / chunks.len() as u32)
    );

    let candidates: Vec<(uuid::Uuid, Vec<f32>)> = chunks
        .iter()
        .map(|_| uuid::Uuid::new_v4())
        .zip(vectors.iter().cloned())
        .collect();
    let start = Instant::now();
    let query_vec = rt.block_on(embedder.embed(query)).unwrap();
    let results = top_k(&query_vec, &candidates, 5, 0.0);
    let query_elapsed = start.elapsed();
    println!(
        "query (embed + top-5 over {} chunks): {}",
        candidates.len(),
        human_ms(query_elapsed)
    );
    println!(
        "top match score: {:.4}",
        results.first().map(|r| r.score).unwrap_or(0.0)
    );
}

fn main() {
    println!("Aside RAG benchmark (status.md 7.1) — Apple Silicon, real product code, no synthetic timers.");
    let segments = synthetic_transcript_segments();
    let query = "What visa status do you currently hold, and would you need sponsorship?";

    bench_tfidf(&segments, query);
    bench_onnx(&segments, query);

    println!(
        "\n(c) Cloud embeddings API: not benchmarked with a real network call this session — no API key \
         available. Reasoned about instead from this project's own already-measured cloud round-trip \
         precedent (Deepgram/Speechmatics/OpenAI et al. — typically 50-300ms per HTTP call from a \
         residential/office connection, dominated by network latency, not server-side compute) — see \
         docs/evidence.md for the full reasoning."
    );
}
