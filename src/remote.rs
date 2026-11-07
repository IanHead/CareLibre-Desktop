//! Background worker for server-backed contact data.
//!
//! Keeping network calls off the UI thread means opening Contacts or a record
//! never blocks painting, even on a slow link.

use std::{thread, time::Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;

use crate::{
    api::ApiClient,
    contact::{ApiContactDetail, ApiContactList, ApiContactWrite},
    store::{ContactCursor, ContactFilter},
};

pub enum RemoteCommand {
    ListContacts {
        request_id: u64,
        base_url: String,
        token: String,
        filter: ContactFilter,
        role: &'static str,
        search: String,
        after: Option<ContactCursor>,
        limit: usize,
        append: bool,
    },
    LoadContact {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    },
    SaveContact {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: Box<ApiContactWrite>,
    },
    CreateContact {
        request_id: u64,
        base_url: String,
        token: String,
        payload: Box<ApiContactWrite>,
    },
}

pub enum RemoteEvent {
    ContactsListed {
        request_id: u64,
        filter: ContactFilter,
        append: bool,
        result: Result<(ApiContactList, f32), String>,
    },
    ContactLoaded {
        request_id: u64,
        id: String,
        result: Result<ApiContactDetail, String>,
    },
    ContactSaved {
        request_id: u64,
        id: String,
        result: Result<ApiContactDetail, String>,
    },
    ContactCreated {
        request_id: u64,
        result: Result<ApiContactDetail, String>,
    },
}

pub struct RemoteHandle {
    commands: Sender<RemoteCommand>,
    events: Receiver<RemoteEvent>,
}

impl RemoteHandle {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (commands, command_rx) = unbounded();
        let (event_tx, events) = unbounded();

        thread::Builder::new()
            .name("carelibre-remote".to_owned())
            .spawn(move || run(ctx, command_rx, event_tx))
            .expect("failed to spawn remote worker thread");

        Self { commands, events }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn list_contacts(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        filter: ContactFilter,
        role: &'static str,
        search: String,
        after: Option<ContactCursor>,
        limit: usize,
        append: bool,
    ) {
        let _ = self.commands.send(RemoteCommand::ListContacts {
            request_id,
            base_url,
            token,
            filter,
            role,
            search,
            after,
            limit,
            append,
        });
    }

    pub fn load_contact(&self, request_id: u64, base_url: String, token: String, id: String) {
        let _ = self.commands.send(RemoteCommand::LoadContact {
            request_id,
            base_url,
            token,
            id,
        });
    }

    pub fn save_contact(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: ApiContactWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::SaveContact {
            request_id,
            base_url,
            token,
            id,
            payload: Box::new(payload),
        });
    }

    pub fn create_contact(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        payload: ApiContactWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateContact {
            request_id,
            base_url,
            token,
            payload: Box::new(payload),
        });
    }

    pub fn try_recv(&self) -> Option<RemoteEvent> {
        self.events.try_recv().ok()
    }
}

fn run(ctx: egui::Context, commands: Receiver<RemoteCommand>, events: Sender<RemoteEvent>) {
    while let Ok(command) = commands.recv() {
        match command {
            RemoteCommand::ListContacts {
                request_id,
                base_url,
                token,
                filter,
                role,
                search,
                after,
                limit,
                append,
            } => {
                let started = Instant::now();
                let after_ref = after
                    .as_ref()
                    .map(|cursor| (cursor.name.as_str(), cursor.id.as_str()));
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.list_contacts(&token, role, &search, after_ref, limit)
                    })
                    .map(|list| (list, started.elapsed().as_secs_f32() * 1000.0))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ContactsListed {
                    request_id,
                    filter,
                    append,
                    result,
                });
            }
            RemoteCommand::LoadContact {
                request_id,
                base_url,
                token,
                id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.get_contact(&token, &id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ContactLoaded {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::SaveContact {
                request_id,
                base_url,
                token,
                id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.update_contact(&token, &id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ContactSaved {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::CreateContact {
                request_id,
                base_url,
                token,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_contact(&token, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ContactCreated { request_id, result });
            }
        }

        ctx.request_repaint();
    }
}
