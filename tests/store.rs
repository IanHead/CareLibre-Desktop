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
        counts.clients + counts.workers + counts.dual + counts.other,
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

#[test]
fn intake_drafts_round_trip_and_complete() {
    use carelibre_desktop::intake::{IntakeDraft, IntakeKind};

    let mut conn = open_in_memory().expect("in-memory database");

    let mut draft = IntakeDraft::new(IntakeKind::Client);
    draft.set("first_name", "Margaret".to_owned());
    draft.set("last_name", "Chen".to_owned());
    draft.set("email", "margaret@example.com".to_owned());
    draft.set("phone", "0400 000 000".to_owned());
    draft.set("suburb", "Newtown".to_owned());
    draft.current_step = 2;

    carelibre_desktop::store::save_intake(&conn, DEMO_ORG_ID, draft.clone()).expect("save draft");

    let listed = carelibre_desktop::store::list_intakes(&conn, DEMO_ORG_ID).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Margaret Chen");
    assert_eq!(listed[0].current_step, 2);

    let loaded = carelibre_desktop::store::load_intake(&conn, DEMO_ORG_ID, &draft.id)
        .expect("load")
        .expect("draft");
    assert_eq!(loaded.get("first_name"), "Margaret");

    let completed = carelibre_desktop::store::complete_intake(&mut conn, DEMO_ORG_ID, loaded)
        .expect("complete");
    assert_eq!(completed.name, "Margaret Chen");

    let listed = carelibre_desktop::store::list_intakes(&conn, DEMO_ORG_ID).expect("list after");
    assert!(listed.is_empty(), "completed intakes leave the draft list");

    let kind: String = conn
        .query_row(
            "SELECT kind FROM contacts WHERE id = ?1",
            [&completed.contact_id],
            |row| row.get(0),
        )
        .expect("contact exists");
    assert_eq!(kind, "client");
}

#[test]
fn other_contacts_complete_as_other_kind() {
    use carelibre_desktop::intake::{IntakeDraft, IntakeKind};

    let mut conn = open_in_memory().expect("in-memory database");
    let mut draft = IntakeDraft::new(IntakeKind::Other);
    draft.set("first_name", "Priya".to_owned());
    draft.set("last_name", "Nair".to_owned());
    draft.set("phone", "0400 111 222".to_owned());

    let completed = carelibre_desktop::store::complete_intake(&mut conn, DEMO_ORG_ID, draft)
        .expect("complete other intake");

    let kind: String = conn
        .query_row(
            "SELECT kind FROM contacts WHERE id = ?1",
            [&completed.contact_id],
            |row| row.get(0),
        )
        .expect("contact exists");
    assert_eq!(kind, "other");
}
