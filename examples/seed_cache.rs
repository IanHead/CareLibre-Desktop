//! Seed the local SQLite cache with deterministic demo data.
//!
//! ```sh
//! cargo run --release --example seed_cache -- 50000 1000000
//! ```

use carelibre_desktop::store::{self, DEMO_ORG_ID};

fn main() {
    let mut args = std::env::args().skip(1);
    let contacts: usize = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(50_000);
    let shifts: usize = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1_000_000);

    let path = store::default_cache_path().expect("a platform data directory");
    let mut conn = store::open(&path).expect("open the local cache");
    let report =
        store::seed_demo(&mut conn, DEMO_ORG_ID, contacts, shifts).expect("seed demo data");

    println!(
        "Seeded {} contacts and {} shifts into {} in {:.0} ms",
        report.contacts,
        report.shifts,
        path.display(),
        report.duration_ms
    );
}
