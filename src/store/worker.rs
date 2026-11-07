//! Background worker that owns the SQLite connection used for reads and demo seeding.

use std::{path::PathBuf, thread};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;

use crate::intake::{CompletedIntake, IntakeDraft, IntakeSummary};

use super::{
    ContactCounts, ContactCursor, ContactFilter, ContactPage, DEMO_ORG_ID, SeedReport, intake,
    open, query, seed,
};

enum Command {
    Query {
        generation: u64,
        filter: ContactFilter,
        search: String,
        after: Option<ContactCursor>,
        limit: usize,
    },
    SeedDemo {
        contacts: usize,
        shifts: usize,
    },
    ListIntakes {
        request_id: u64,
    },
    LoadIntake {
        request_id: u64,
        id: String,
    },
    SaveIntake {
        draft: Box<IntakeDraft>,
    },
    CompleteIntake {
        draft: Box<IntakeDraft>,
    },
    DiscardIntake {
        request_id: u64,
        id: String,
    },
}

pub enum StoreEvent {
    Ready {
        path: String,
        counts: ContactCounts,
        shifts: usize,
    },
    Failed {
        error: String,
    },
    Contacts {
        generation: u64,
        filter: ContactFilter,
        append: bool,
        page: Result<ContactPage, String>,
    },
    Seeded {
        result: Result<SeedReport, String>,
        counts: ContactCounts,
        shifts: usize,
    },
    IntakesListed {
        request_id: u64,
        result: Result<Vec<IntakeSummary>, String>,
    },
    IntakeLoaded {
        request_id: u64,
        result: Result<Option<IntakeDraft>, String>,
    },
    IntakeSaved {
        result: Result<(), String>,
    },
    IntakeCompleted {
        result: Result<CompletedIntake, String>,
    },
    IntakeDiscarded {
        request_id: u64,
        id: String,
        result: Result<(), String>,
    },
}

/// Handle to the local cache worker.
pub struct StoreHandle {
    commands: Sender<Command>,
    events: Receiver<StoreEvent>,
}

impl StoreHandle {
    pub fn spawn(path: PathBuf, ctx: egui::Context) -> Self {
        let (commands, command_rx) = unbounded();
        let (event_tx, events) = unbounded();

        thread::Builder::new()
            .name("carelibre-store".to_owned())
            .spawn(move || run(path, ctx, command_rx, event_tx))
            .expect("failed to spawn store worker thread");

        Self { commands, events }
    }

    pub fn query(
        &self,
        generation: u64,
        filter: ContactFilter,
        search: String,
        after: Option<ContactCursor>,
        limit: usize,
    ) {
        let _ = self.commands.send(Command::Query {
            generation,
            filter,
            search,
            after,
            limit,
        });
    }

    pub fn seed_demo(&self, contacts: usize, shifts: usize) {
        let _ = self.commands.send(Command::SeedDemo { contacts, shifts });
    }

    pub fn list_intakes(&self, request_id: u64) {
        let _ = self.commands.send(Command::ListIntakes { request_id });
    }

    pub fn load_intake(&self, request_id: u64, id: String) {
        let _ = self.commands.send(Command::LoadIntake { request_id, id });
    }

    pub fn save_intake(&self, draft: IntakeDraft) {
        let _ = self.commands.send(Command::SaveIntake {
            draft: Box::new(draft),
        });
    }

    pub fn complete_intake(&self, draft: IntakeDraft) {
        let _ = self.commands.send(Command::CompleteIntake {
            draft: Box::new(draft),
        });
    }

    pub fn discard_intake(&self, request_id: u64, id: String) {
        let _ = self
            .commands
            .send(Command::DiscardIntake { request_id, id });
    }

    pub fn try_recv(&self) -> Option<StoreEvent> {
        self.events.try_recv().ok()
    }
}

fn run(path: PathBuf, ctx: egui::Context, commands: Receiver<Command>, events: Sender<StoreEvent>) {
    let mut conn = match open(&path) {
        Ok(conn) => conn,
        Err(error) => {
            let _ = events.send(StoreEvent::Failed {
                error: format!("Could not open the local cache: {error}"),
            });
            ctx.request_repaint();
            return;
        }
    };

    let mut counts = query::contact_counts(&conn, DEMO_ORG_ID).unwrap_or_default();
    let shifts = query::count_shifts_in_range(&conn, DEMO_ORG_ID, 0, i64::MAX).unwrap_or(0);
    let _ = events.send(StoreEvent::Ready {
        path: path.display().to_string(),
        counts,
        shifts,
    });
    ctx.request_repaint();

    let mut last_total: Option<(ContactFilter, String, usize)> = None;

    while let Ok(command) = commands.recv() {
        match command {
            Command::Query {
                generation,
                filter,
                search,
                after,
                limit,
            } => {
                let append = after.is_some();
                let known_total = if append {
                    last_total
                        .as_ref()
                        .filter(|(cached_filter, cached_search, _)| {
                            *cached_filter == filter && cached_search == &search
                        })
                        .map(|(_, _, total)| *total)
                } else if search.trim().is_empty() {
                    Some(counts.for_filter(filter))
                } else {
                    None
                };

                let page = query::query_contacts_with(
                    &conn,
                    DEMO_ORG_ID,
                    filter,
                    &search,
                    after.as_ref(),
                    limit,
                    counts,
                    known_total,
                )
                .map_err(|error| error.to_string());

                if let Ok(page) = &page
                    && !append
                {
                    last_total = Some((filter, search.clone(), page.total));
                }

                let _ = events.send(StoreEvent::Contacts {
                    generation,
                    filter,
                    append,
                    page,
                });
                ctx.request_repaint();
            }
            Command::ListIntakes { request_id } => {
                let result = intake::list_intakes(&conn, DEMO_ORG_ID);
                let _ = events.send(StoreEvent::IntakesListed { request_id, result });
            }
            Command::LoadIntake { request_id, id } => {
                let result = intake::load_intake(&conn, DEMO_ORG_ID, &id);
                let _ = events.send(StoreEvent::IntakeLoaded { request_id, result });
            }
            Command::SaveIntake { draft } => {
                let result = intake::save_intake(&conn, DEMO_ORG_ID, *draft);
                let _ = events.send(StoreEvent::IntakeSaved { result });
            }
            Command::CompleteIntake { draft } => {
                let result = intake::complete_intake(&mut conn, DEMO_ORG_ID, *draft);
                let _ = events.send(StoreEvent::IntakeCompleted { result });
            }
            Command::DiscardIntake { request_id, id } => {
                let result = intake::delete_intake(&conn, DEMO_ORG_ID, &id);
                let _ = events.send(StoreEvent::IntakeDiscarded {
                    request_id,
                    id,
                    result,
                });
            }
            Command::SeedDemo { contacts, shifts } => {
                let result = seed::seed_demo(&mut conn, DEMO_ORG_ID, contacts, shifts)
                    .map_err(|error| error.to_string());
                counts = query::contact_counts(&conn, DEMO_ORG_ID).unwrap_or_default();
                last_total = None;
                let shifts =
                    query::count_shifts_in_range(&conn, DEMO_ORG_ID, 0, i64::MAX).unwrap_or(0);
                let _ = events.send(StoreEvent::Seeded {
                    result,
                    counts,
                    shifts,
                });
                ctx.request_repaint();
            }
        }
    }
}
