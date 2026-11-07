//! Server connection state and the background worker that talks to the API.
//!
//! HTTP calls are blocking, so they run on a dedicated thread. The UI sends
//! commands and receives events over channels; the worker wakes egui whenever
//! an event is ready.

use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};

use eframe::egui;

use crate::api::{ApiClient, AuthSession, ServerInfo};

const CHECK_INTERVAL: Duration = Duration::from_secs(15);
const REFRESH_MARGIN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// No worker is running (used by tests and headless previews).
    NotConnected,
    Connecting,
    Online {
        version: String,
        database_up: bool,
        ready_error: Option<String>,
    },
    Offline {
        error: String,
    },
}

#[derive(Clone)]
pub struct Session {
    pub auth: AuthSession,
    pub expires_at: Instant,
}

impl Session {
    pub fn first_name(&self) -> &str {
        self.auth
            .user
            .full_name
            .split_whitespace()
            .next()
            .unwrap_or("")
    }

    pub fn initials(&self) -> String {
        let mut initials = self
            .auth
            .user
            .full_name
            .split_whitespace()
            .filter_map(|part| part.chars().next())
            .take(2)
            .collect::<String>();
        if initials.is_empty() {
            initials.push('?');
        }
        initials.make_ascii_uppercase();
        initials
    }

    pub fn role(&self) -> &str {
        &self.auth.organisation.role
    }

    pub fn needs_refresh(&self) -> bool {
        self.expires_at.saturating_duration_since(Instant::now()) <= REFRESH_MARGIN
    }
}

pub struct ServerState {
    pub base_url: String,
    pub connection: ConnectionState,
    pub session: Option<Session>,
    pub last_checked: Option<Instant>,
    pub auth_in_flight: bool,
    pub refresh_in_flight: bool,
}

impl ServerState {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            connection: ConnectionState::NotConnected,
            session: None,
            last_checked: None,
            auth_in_flight: false,
            refresh_in_flight: false,
        }
    }

    pub fn is_online(&self) -> bool {
        matches!(self.connection, ConnectionState::Online { .. })
    }
}

#[derive(Debug)]
enum Command {
    SetBaseUrl(String),
    Check,
    Login {
        email: String,
        password: String,
        organisation_slug: Option<String>,
    },
    Register {
        organisation_name: String,
        organisation_slug: String,
        full_name: String,
        email: String,
        password: String,
    },
    Refresh {
        refresh_token: String,
    },
    Logout {
        refresh_token: String,
    },
}

pub enum Event {
    Status {
        base_url: String,
        result: Result<ServerInfo, String>,
    },
    Authenticated {
        base_url: String,
        result: Result<AuthSession, String>,
    },
    Refreshed {
        base_url: String,
        result: Result<AuthSession, String>,
    },
    LoggedOut {
        result: Result<(), String>,
    },
}

/// Handle to the background server worker.
pub struct ServerRuntime {
    commands: Sender<Command>,
    events: Receiver<Event>,
}

impl ServerRuntime {
    pub fn spawn(ctx: egui::Context, base_url: String) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();

        thread::Builder::new()
            .name("carelibre-server".to_owned())
            .spawn(move || worker(ctx, base_url, command_rx, event_tx))
            .expect("failed to spawn server worker thread");

        Self { commands, events }
    }

    pub fn set_base_url(&self, base_url: impl Into<String>) {
        self.send(Command::SetBaseUrl(base_url.into()));
    }

    pub fn check(&self) {
        self.send(Command::Check);
    }

    pub fn login(&self, email: String, password: String, organisation_slug: Option<String>) {
        self.send(Command::Login {
            email,
            password,
            organisation_slug,
        });
    }

    pub fn register(
        &self,
        organisation_name: String,
        organisation_slug: String,
        full_name: String,
        email: String,
        password: String,
    ) {
        self.send(Command::Register {
            organisation_name,
            organisation_slug,
            full_name,
            email,
            password,
        });
    }

    pub fn refresh(&self, refresh_token: String) {
        self.send(Command::Refresh { refresh_token });
    }

    pub fn logout(&self, refresh_token: String) {
        self.send(Command::Logout { refresh_token });
    }

    pub fn try_recv(&self) -> Option<Event> {
        self.events.try_recv().ok()
    }

    fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }
}

fn worker(
    ctx: egui::Context,
    mut base_url: String,
    commands: Receiver<Command>,
    events: Sender<Event>,
) {
    let mut next_check = Instant::now();

    loop {
        let wait = next_check.saturating_duration_since(Instant::now());

        match commands.recv_timeout(wait) {
            Ok(Command::SetBaseUrl(url)) => {
                base_url = url;
                check(&base_url, &events);
                next_check = Instant::now() + CHECK_INTERVAL;
            }
            Ok(Command::Check) => {
                check(&base_url, &events);
                next_check = Instant::now() + CHECK_INTERVAL;
            }
            Ok(Command::Login {
                email,
                password,
                organisation_slug,
            }) => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.login(&email, &password, organisation_slug.as_deref())
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(Event::Authenticated {
                    base_url: base_url.clone(),
                    result,
                });
            }
            Ok(Command::Register {
                organisation_name,
                organisation_slug,
                full_name,
                email,
                password,
            }) => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.register(
                            &organisation_name,
                            &organisation_slug,
                            &full_name,
                            &email,
                            &password,
                        )
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(Event::Authenticated {
                    base_url: base_url.clone(),
                    result,
                });
            }
            Ok(Command::Refresh { refresh_token }) => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.refresh(&refresh_token))
                    .map_err(|err| err.to_string());
                let _ = events.send(Event::Refreshed {
                    base_url: base_url.clone(),
                    result,
                });
            }
            Ok(Command::Logout { refresh_token }) => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.logout(&refresh_token))
                    .map_err(|err| err.to_string());
                let _ = events.send(Event::LoggedOut { result });
            }
            Err(RecvTimeoutError::Timeout) => {
                check(&base_url, &events);
                next_check = Instant::now() + CHECK_INTERVAL;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }

        ctx.request_repaint();
    }
}

fn check(base_url: &str, events: &Sender<Event>) {
    let result = ApiClient::new(base_url)
        .and_then(|client| client.health())
        .map_err(|err| err.to_string());

    let _ = events.send(Event::Status {
        base_url: base_url.to_owned(),
        result,
    });
}
