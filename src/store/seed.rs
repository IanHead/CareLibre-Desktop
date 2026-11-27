//! Deterministic synthetic data for performance testing.

use std::collections::HashSet;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};

use super::ContactKind;

#[derive(Clone, Copy, Debug)]
pub struct SeedReport {
    pub contacts: usize,
    pub shifts: usize,
    pub duration_ms: f32,
}

const FIRST_NAMES: &[&str] = &[
    "Margaret",
    "Noah",
    "Priya",
    "Sam",
    "Olivia",
    "Liam",
    "Ava",
    "Ethan",
    "Mia",
    "Lucas",
    "Ruby",
    "Jack",
    "Chloe",
    "Leo",
    "Grace",
    "Henry",
    "Zoe",
    "Oscar",
    "Ivy",
    "Mason",
    "Amelia",
    "Logan",
    "Harper",
    "Elijah",
    "Willow",
    "James",
    "Sienna",
    "William",
    "Matilda",
    "Thomas",
    "Charlotte",
    "Oliver",
    "Isla",
    "Ella",
    "Lily",
    "Sophie",
    "Emily",
    "Hannah",
    "Jasmine",
    "Holly",
    "Evie",
    "Scarlett",
    "Isabella",
    "Gabriella",
    "Elena",
    "Francesca",
    "Anna",
    "Marco",
    "Luca",
    "Matteo",
    "Alessandro",
    "Georgia",
    "Penelope",
    "Alexandra",
    "Katerina",
    "Minh",
    "Linh",
    "Wei",
    "Anjali",
    "Rahul",
];

const LAST_NAMES: &[&str] = &[
    "Chen", "Williams", "Nair", "Taylor", "Brown", "Wilson", "Nguyen", "Martin", "Anderson",
    "Thompson", "White", "Walker", "Harris", "Ryan", "Kelly", "Singh", "Patel", "Lee", "Wong",
    "Davis", "Miller", "Moore", "Clark", "Lewis", "Young", "Hall", "Allen", "King", "Wright",
    "Scott", "Johnson", "Smith", "Jones", "Robinson", "Campbell", "Cooper", "Murphy", "Ward",
    "Morris", "Baker", "Stewart", "Watson", "Adams", "Bell", "Collins", "Parker", "Russell",
    "Phillips", "Morgan", "Carter", "Graham", "Cook", "Shaw", "Murray", "Kennedy", "Simpson",
    "Ellis", "Marshall", "Gray", "Stevens",
];

const DIVISIONS: &[&str] = &[
    "Community Care",
    "Aged Care",
    "Disability Services",
    "Respite",
    "Allied Health",
];

const STATUSES: &[&str] = &[
    "Active",
    "Active",
    "Active",
    "Active",
    "Onboarding",
    "Inactive",
];

struct Rng(u64);

impl Rng {
    fn new() -> Self {
        Self(0x9E37_79B9_7F4A_7C15)
    }

    fn next(&mut self) -> u64 {
        // SplitMix64: good distribution in the low bits, unlike a plain LCG.
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    fn pick<'a, T>(&mut self, values: &'a [T]) -> &'a T {
        &values[(self.next() as usize) % values.len()]
    }
}

const MIDDLE_INITIALS: usize = 26;

/// Attempts before falling back to a numeric suffix. Reaching the limit needs
/// a contact list far larger than the 50,000-row performance fixture.
const MAX_NAME_ATTEMPTS: usize = 64;

/// A full name that no other demo contact has used.
///
/// The first name and surname are drawn at random, while the middle initial is
/// tied to the contact's position and shifted on a clash. The numeric suffix is
/// a last resort for lists that exhaust every first, surname and initial.
fn unique_name(
    rng: &mut Rng,
    used: &mut HashSet<String>,
    index: usize,
) -> (String, &'static str, &'static str) {
    for attempt in 0..MAX_NAME_ATTEMPTS {
        let first = *rng.pick(FIRST_NAMES);
        let last = *rng.pick(LAST_NAMES);
        let middle = (b'A' + ((index + attempt) % MIDDLE_INITIALS) as u8) as char;
        let name = format!("{first} {middle}. {last}");
        if used.insert(name.clone()) {
            return (name, first, last);
        }
    }

    let first = *rng.pick(FIRST_NAMES);
    let last = *rng.pick(LAST_NAMES);
    let middle = (b'A' + (index % MIDDLE_INITIALS) as u8) as char;
    let name = format!("{first} {middle}. {last} {}", index + 1);
    used.insert(name.clone());
    (name, first, last)
}

pub fn seed_demo(
    conn: &mut Connection,
    org_id: &str,
    contacts: usize,
    shifts: usize,
) -> rusqlite::Result<SeedReport> {
    let started = Instant::now();
    let mut rng = Rng::new();
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);

    let tx = conn.transaction()?;
    tx.execute("DELETE FROM shifts WHERE org_id = ?1", [org_id])?;
    tx.execute("DELETE FROM contacts WHERE org_id = ?1", [org_id])?;

    {
        let mut insert = tx.prepare(
            "INSERT INTO contacts
                (id, org_id, name, kind, email, phone, division, status, updated_at, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1)",
        )?;
        let mut used_names = HashSet::with_capacity(contacts);

        for index in 0..contacts {
            let kind = match rng.next() % 100 {
                0..=64 => ContactKind::Client,
                65..=89 => ContactKind::Worker,
                _ => ContactKind::Dual,
            };
            let (name, first, last) = unique_name(&mut rng, &mut used_names, index);
            let email = format!(
                "{}.{}{:05}@example.com",
                first.to_lowercase(),
                last.to_lowercase(),
                index % 100_000
            );
            let phone = format!(
                "04{:02} {:03} {:03}",
                rng.next() % 100,
                rng.next() % 1000,
                rng.next() % 1000
            );
            let division = *rng.pick(DIVISIONS);
            let status = *rng.pick(STATUSES);

            insert.execute(params![
                format!("demo-contact-{index:07}"),
                org_id,
                name,
                kind.as_db(),
                email,
                phone,
                division,
                status,
                now_ms,
            ])?;
        }
    }

    if contacts > 0 && shifts > 0 {
        let mut insert = tx.prepare(
            "INSERT INTO shifts
                (id, org_id, worker_id, client_id, starts_at, ends_at, status, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
        )?;

        let base = now_ms - 180 * 24 * 60 * 60 * 1000;
        for index in 0..shifts {
            let worker = rng.next() as usize % contacts;
            let mut client = rng.next() as usize % contacts;
            if client == worker && contacts > 1 {
                client = (client + 1) % contacts;
            }

            let day = rng.next() as i64 % 365;
            let hour = 6 + (rng.next() % 14) as i64;
            let minute = (rng.next() % 4) as i64 * 15;
            let starts_at =
                base + day * 24 * 60 * 60 * 1000 + hour * 60 * 60 * 1000 + minute * 60 * 1000;
            let duration = 1 + (rng.next() % 8) as i64;
            let ends_at = starts_at + duration * 60 * 60 * 1000;
            let status = match rng.next() % 100 {
                0..=69 => "Scheduled",
                70..=94 => "Completed",
                _ => "Cancelled",
            };

            insert.execute(params![
                format!("demo-shift-{index:09}"),
                org_id,
                format!("demo-contact-{worker:07}"),
                format!("demo-contact-{client:07}"),
                starts_at,
                ends_at,
                status,
            ])?;
        }
    }

    tx.commit()?;

    Ok(SeedReport {
        contacts,
        shifts,
        duration_ms: started.elapsed().as_secs_f32() * 1000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_names_are_unique_at_fixture_scale() {
        let mut rng = Rng::new();
        let mut used = HashSet::new();
        let names: Vec<String> = (0..50_000)
            .map(|index| unique_name(&mut rng, &mut used, index).0)
            .collect();
        let distinct: HashSet<&String> = names.iter().collect();

        assert_eq!(
            distinct.len(),
            names.len(),
            "demo contact names should not repeat"
        );
    }
}
