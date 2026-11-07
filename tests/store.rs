use carelibre_desktop::store::{
    ContactFilter, DEMO_ORG_ID, contact_counts, count_shifts_in_range, open_in_memory,
    query_contacts, seed_demo, shifts_in_range,
};

fn seeded() -> rusqlite::Connection {
    let mut conn = open_in_memory().expect("in-memory database");
    seed_demo(&mut conn, DEMO_ORG_ID, 250, 1_000).expect("seed demo data");
    conn
}

#[test]
fn counts_contacts_by_role() {
    let conn = seeded();
    let counts = contact_counts(&conn, DEMO_ORG_ID).expect("counts");

    assert_eq!(counts.all, 250);
    assert_eq!(
        counts.clients + counts.workers + counts.dual,
        counts.all,
        "roles should partition the contact table"
    );
}

#[test]
fn pages_contacts_without_duplicates() {
    let conn = seeded();

    let first =
        query_contacts(&conn, DEMO_ORG_ID, ContactFilter::All, "", None, 50).expect("first page");
    assert_eq!(first.rows.len(), 50);
    assert_eq!(first.total, 250);
    assert!(first.next_cursor.is_some());

    let cursor = first.next_cursor.as_ref().expect("cursor");
    let second = query_contacts(&conn, DEMO_ORG_ID, ContactFilter::All, "", Some(cursor), 50)
        .expect("second page");

    assert_eq!(second.rows.len(), 50);
    let first_ids: std::collections::HashSet<_> = first.rows.iter().map(|row| &row.id).collect();
    assert!(
        second.rows.iter().all(|row| !first_ids.contains(&row.id)),
        "keyset pagination must not repeat rows"
    );
}

#[test]
fn filters_clients_and_workers() {
    let conn = seeded();
    let counts = contact_counts(&conn, DEMO_ORG_ID).expect("counts");

    let clients = query_contacts(&conn, DEMO_ORG_ID, ContactFilter::Clients, "", None, 500)
        .expect("client page");
    assert_eq!(clients.total, counts.clients + counts.dual);

    let workers = query_contacts(&conn, DEMO_ORG_ID, ContactFilter::Workers, "", None, 500)
        .expect("worker page");
    assert_eq!(workers.total, counts.workers + counts.dual);
}

#[test]
fn full_text_search_matches_names() {
    let conn = seeded();
    let first =
        query_contacts(&conn, DEMO_ORG_ID, ContactFilter::All, "", None, 1).expect("first page");
    let name = first.rows[0].name.clone();
    let term = name.split_whitespace().next().expect("first name");

    let results = query_contacts(&conn, DEMO_ORG_ID, ContactFilter::All, term, None, 100)
        .expect("search results");

    assert!(results.total > 0, "expected a match for {term}");
    assert!(
        results
            .rows
            .iter()
            .all(|row| row.name.to_lowercase().contains(&term.to_lowercase())),
        "every result should contain the search term"
    );
}

#[test]
fn seeds_and_queries_shifts() {
    let conn = seeded();
    let from = 0;
    let to = i64::MAX / 2;

    let count = count_shifts_in_range(&conn, DEMO_ORG_ID, from, to).expect("shift count");
    assert_eq!(count, 1_000);

    let shifts = shifts_in_range(&conn, DEMO_ORG_ID, from, to, 100).expect("shift page");
    assert_eq!(shifts.len(), 100);
}

#[test]
fn reseeding_replaces_the_previous_demo_data() {
    let mut conn = seeded();
    seed_demo(&mut conn, DEMO_ORG_ID, 10, 20).expect("reseed");

    let counts = contact_counts(&conn, DEMO_ORG_ID).expect("counts");
    assert_eq!(counts.all, 10);
}
