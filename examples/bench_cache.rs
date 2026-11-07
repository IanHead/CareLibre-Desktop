//! Time local cache reads against the seeded database.
//!
//! ```sh
//! cargo run --release --example bench_cache
//! ```

use std::time::Instant;

use carelibre_desktop::store::{self, ContactFilter, DEMO_ORG_ID};

fn main() {
    let path = store::default_cache_path().expect("a platform data directory");
    let conn = store::open(&path).expect("open the local cache");

    let counts = store::contact_counts(&conn, DEMO_ORG_ID).expect("contact counts");
    println!(
        "contacts: {} ({} clients, {} workers, {} dual)",
        counts.all, counts.clients, counts.workers, counts.dual
    );

    let mut samples = Vec::new();
    for _ in 0..50 {
        let started = Instant::now();
        let page = store::query_contacts(&conn, DEMO_ORG_ID, ContactFilter::Clients, "", None, 200)
            .expect("contact page");
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(page.rows.len(), 200);
    }
    report("clients first page", &mut samples);

    let mut samples = Vec::new();
    for _ in 0..50 {
        let started = Instant::now();
        let page = store::query_contacts(
            &conn,
            DEMO_ORG_ID,
            ContactFilter::All,
            "margaret",
            None,
            200,
        )
        .expect("search page");
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(!page.rows.is_empty());
    }
    report("FTS search page", &mut samples);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let week = 7 * 24 * 60 * 60 * 1000;
    let from = now - 30 * 24 * 60 * 60 * 1000;

    let mut samples = Vec::new();
    for _ in 0..50 {
        let started = Instant::now();
        let count = store::count_shifts_in_range(&conn, DEMO_ORG_ID, from, from + week)
            .expect("shift count");
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(count > 0);
    }
    report("roster week count", &mut samples);

    let mut samples = Vec::new();
    for _ in 0..50 {
        let started = Instant::now();
        let shifts = store::shifts_in_range(&conn, DEMO_ORG_ID, from, from + week, 2_000)
            .expect("shift page");
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        assert!(!shifts.is_empty());
    }
    report("roster week rows (2k cap)", &mut samples);
}

fn report(label: &str, samples: &mut [f64]) {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p50 = samples[samples.len() / 2];
    let p95 = samples[(samples.len() * 95 / 100).min(samples.len() - 1)];
    let worst = samples.last().copied().unwrap_or(0.0);
    println!("{label:<26} p50 {p50:>6.2} ms   p95 {p95:>6.2} ms   worst {worst:>6.2} ms");
}
