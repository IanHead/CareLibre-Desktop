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
    use carelibre_desktop::contact::PartyKind;
    use carelibre_desktop::intake::{IntakeDraft, IntakeKind, IntakeRelationship};

    let mut conn = open_in_memory().expect("in-memory database");

    let mut draft = IntakeDraft::new(IntakeKind::Client);
    draft.set("first_name", "Margaret".to_owned());
    draft.set("last_name", "Chen".to_owned());
    draft.set("email", "margaret@example.com".to_owned());
    draft.set("phone", "0400 000 000".to_owned());
    draft.set("suburb", "Newtown".to_owned());
    draft.current_step = 2;
    draft.set_relationship(
        "emergency_contact",
        IntakeRelationship {
            subject_role: "person_supported".to_owned(),
            linked_role: "emergency_contact".to_owned(),
            kind: PartyKind::Contact,
            id: "contact-1".to_owned(),
            name: "Alex Chen".to_owned(),
            notes: "Daughter".to_owned(),
        },
    );

    carelibre_desktop::store::save_intake(&conn, DEMO_ORG_ID, draft.clone()).expect("save draft");

    let listed = carelibre_desktop::store::list_intakes(&conn, DEMO_ORG_ID).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Margaret Chen");
    assert_eq!(listed[0].current_step, 2);

    let loaded = carelibre_desktop::store::load_intake(&conn, DEMO_ORG_ID, &draft.id)
        .expect("load")
        .expect("draft");
    assert_eq!(loaded.get("first_name"), "Margaret");
    assert_eq!(
        loaded.relationship("emergency_contact"),
        draft.relationship("emergency_contact"),
        "selected relationships survive the draft round trip"
    );

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

#[test]
fn loads_and_saves_full_contact_with_custom_fields() {
    use std::collections::BTreeMap;

    use carelibre_desktop::contact::ContactUpdate;
    use carelibre_desktop::store::{ContactKind, list_custom_fields, load_contact, save_contact};

    let mut conn = open_in_memory().expect("in-memory database");
    seed_demo(&mut conn, DEMO_ORG_ID, 5, 0).expect("seed contacts");

    let fields = list_custom_fields(&conn, DEMO_ORG_ID).expect("custom metadata");
    assert!(
        fields.iter().any(|field| field.label == "Medicare number"),
        "demo custom fields should be seeded as metadata"
    );

    let id = "demo-contact-0000000";
    let record = load_contact(&conn, DEMO_ORG_ID, id)
        .expect("load contact")
        .expect("contact exists");
    assert!(record.profile.is_empty());

    let mut profile = BTreeMap::new();
    profile.insert("address_line1".to_owned(), "12 Test Street".to_owned());
    profile.insert("primary_disability".to_owned(), "Autism".to_owned());

    let field_id = fields
        .iter()
        .find(|field| field.key == "medicare_number")
        .expect("medicare field")
        .id
        .clone();
    let mut custom_values = BTreeMap::new();
    custom_values.insert(field_id.clone(), "1234 56789 1".to_owned());

    let update = ContactUpdate {
        id: id.to_owned(),
        name: "Edited Contact".to_owned(),
        kind: ContactKind::Client,
        email: Some("edited@example.com".to_owned()),
        phone: Some("0400 123 456".to_owned()),
        division: Some("Community Care".to_owned()),
        status: "Active".to_owned(),
        profile,
        custom_values,
    };

    save_contact(&mut conn, DEMO_ORG_ID, &update).expect("save contact");

    let loaded = load_contact(&conn, DEMO_ORG_ID, id)
        .expect("reload")
        .expect("contact");
    assert_eq!(loaded.summary.name, "Edited Contact");
    assert_eq!(loaded.summary.kind, ContactKind::Client);
    assert_eq!(loaded.value("address_line1"), "12 Test Street");
    assert_eq!(loaded.custom_value(&field_id), "1234 56789 1");

    let results = query_contacts(&conn, DEMO_ORG_ID, ContactFilter::All, "Edited", None, 10)
        .expect("search edited contact");
    assert_eq!(
        results.total, 1,
        "FTS index should be updated by the trigger"
    );
}
