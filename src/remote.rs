//! Background worker for server-backed contact data.
//!
//! Keeping network calls off the UI thread means opening Contacts or a record
//! never blocks painting, even on a slow link.

use std::{thread, time::Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;

use crate::{
    api::ApiClient,
    contact::{
        ApiContactDetail, ApiContactList, ApiContactWrite, ApiRelationship, ApiRelationshipGraph,
        ApiRelationshipList, ApiRelationshipWrite,
    },
    rostering::{
        ApiAssignment, ApiAssignmentUpdate, ApiAssignmentWrite, ApiBatchShiftWrite,
        ApiCopyShiftWrite, ApiCostCentre, ApiDivision, ApiService, ApiShift, ApiShiftDetail,
        ApiShiftList, ApiShiftUpdate, ApiShiftWrite, ApiSite, ApiSiteWrite, ApiSuitability,
    },
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
    ListRelationships {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: Option<String>,
        organisation_id: Option<String>,
    },
    RelationshipGraph {
        request_id: u64,
        base_url: String,
        token: String,
    },
    CreateRelationship {
        request_id: u64,
        base_url: String,
        token: String,
        payload: Box<ApiRelationshipWrite>,
    },
    DeleteRelationship {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    },
    ListSites {
        request_id: u64,
        base_url: String,
        token: String,
    },
    CreateSite {
        request_id: u64,
        base_url: String,
        token: String,
        payload: Box<ApiSiteWrite>,
    },
    ListShifts {
        request_id: u64,
        base_url: String,
        token: String,
        from: Option<String>,
        to: Option<String>,
        status: Option<String>,
        site_id: Option<String>,
        division_id: Option<String>,
        service_id: Option<String>,
        cost_centre_id: Option<String>,
    },
    RosterFilters {
        request_id: u64,
        base_url: String,
        token: String,
    },
    CreateShifts {
        request_id: u64,
        base_url: String,
        token: String,
        payload: Box<ApiBatchShiftWrite>,
    },
    CopyShift {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: Box<ApiCopyShiftWrite>,
    },
    DeleteShift {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    },
    GetShift {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    },
    CreateShift {
        request_id: u64,
        base_url: String,
        token: String,
        payload: Box<ApiShiftWrite>,
    },
    UpdateShift {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: Box<ApiShiftUpdate>,
    },
    CreateAssignment {
        request_id: u64,
        base_url: String,
        token: String,
        shift_id: String,
        payload: Box<ApiAssignmentWrite>,
    },
    UpdateAssignment {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: Box<ApiAssignmentUpdate>,
    },
    DeleteAssignment {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    },
    ShiftSuitability {
        request_id: u64,
        base_url: String,
        token: String,
        shift_id: String,
    },
}

pub type RosterFiltersResult =
    Result<(Vec<ApiDivision>, Vec<ApiService>, Vec<ApiCostCentre>), String>;

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
    RelationshipsListed {
        request_id: u64,
        contact_id: Option<String>,
        organisation_id: Option<String>,
        result: Result<ApiRelationshipList, String>,
    },
    RelationshipGraphLoaded {
        request_id: u64,
        result: Result<ApiRelationshipGraph, String>,
    },
    RelationshipCreated {
        request_id: u64,
        result: Result<ApiRelationship, String>,
    },
    RelationshipDeleted {
        request_id: u64,
        id: String,
        result: Result<(), String>,
    },
    SitesListed {
        request_id: u64,
        result: Result<Vec<ApiSite>, String>,
    },
    SiteCreated {
        request_id: u64,
        result: Result<ApiSite, String>,
    },
    ShiftsListed {
        request_id: u64,
        result: Result<ApiShiftList, String>,
    },
    RosterFiltersLoaded {
        request_id: u64,
        result: RosterFiltersResult,
    },
    ShiftsCreated {
        request_id: u64,
        result: Result<ApiShiftList, String>,
    },
    ShiftCopied {
        request_id: u64,
        result: Result<ApiShiftDetail, String>,
    },
    ShiftDeleted {
        request_id: u64,
        id: String,
        result: Result<(), String>,
    },
    ShiftLoaded {
        request_id: u64,
        id: String,
        result: Result<ApiShiftDetail, String>,
    },
    ShiftCreated {
        request_id: u64,
        result: Result<ApiShift, String>,
    },
    ShiftUpdated {
        request_id: u64,
        id: String,
        result: Result<ApiShift, String>,
    },
    AssignmentCreated {
        request_id: u64,
        shift_id: String,
        result: Result<ApiAssignment, String>,
    },
    AssignmentUpdated {
        request_id: u64,
        id: String,
        result: Result<ApiAssignment, String>,
    },
    AssignmentDeleted {
        request_id: u64,
        id: String,
        result: Result<(), String>,
    },
    SuitabilityLoaded {
        request_id: u64,
        shift_id: String,
        result: Result<ApiSuitability, String>,
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

    pub fn list_relationships(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: Option<String>,
        organisation_id: Option<String>,
    ) {
        let _ = self.commands.send(RemoteCommand::ListRelationships {
            request_id,
            base_url,
            token,
            contact_id,
            organisation_id,
        });
    }

    pub fn relationship_graph(&self, request_id: u64, base_url: String, token: String) {
        let _ = self.commands.send(RemoteCommand::RelationshipGraph {
            request_id,
            base_url,
            token,
        });
    }

    pub fn create_relationship(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        payload: ApiRelationshipWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateRelationship {
            request_id,
            base_url,
            token,
            payload: Box::new(payload),
        });
    }

    pub fn delete_relationship(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::DeleteRelationship {
            request_id,
            base_url,
            token,
            id,
        });
    }

    pub fn list_sites(&self, request_id: u64, base_url: String, token: String) {
        let _ = self.commands.send(RemoteCommand::ListSites {
            request_id,
            base_url,
            token,
        });
    }

    pub fn create_site(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        payload: ApiSiteWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateSite {
            request_id,
            base_url,
            token,
            payload: Box::new(payload),
        });
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn list_shifts(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        from: Option<String>,
        to: Option<String>,
        status: Option<String>,
        site_id: Option<String>,
        division_id: Option<String>,
        service_id: Option<String>,
        cost_centre_id: Option<String>,
    ) {
        let _ = self.commands.send(RemoteCommand::ListShifts {
            request_id,
            base_url,
            token,
            from,
            to,
            status,
            site_id,
            division_id,
            service_id,
            cost_centre_id,
        });
    }

    pub fn roster_filters(&self, request_id: u64, base_url: String, token: String) {
        let _ = self.commands.send(RemoteCommand::RosterFilters {
            request_id,
            base_url,
            token,
        });
    }

    pub fn create_shifts(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        payload: ApiBatchShiftWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateShifts {
            request_id,
            base_url,
            token,
            payload: Box::new(payload),
        });
    }

    pub fn copy_shift(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: ApiCopyShiftWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CopyShift {
            request_id,
            base_url,
            token,
            id,
            payload: Box::new(payload),
        });
    }

    pub fn delete_shift(&self, request_id: u64, base_url: String, token: String, id: String) {
        let _ = self.commands.send(RemoteCommand::DeleteShift {
            request_id,
            base_url,
            token,
            id,
        });
    }

    pub fn get_shift(&self, request_id: u64, base_url: String, token: String, id: String) {
        let _ = self.commands.send(RemoteCommand::GetShift {
            request_id,
            base_url,
            token,
            id,
        });
    }

    pub fn create_shift(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        payload: ApiShiftWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateShift {
            request_id,
            base_url,
            token,
            payload: Box::new(payload),
        });
    }

    pub fn update_shift(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: ApiShiftUpdate,
    ) {
        let _ = self.commands.send(RemoteCommand::UpdateShift {
            request_id,
            base_url,
            token,
            id,
            payload: Box::new(payload),
        });
    }

    pub fn create_assignment(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        shift_id: String,
        payload: ApiAssignmentWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateAssignment {
            request_id,
            base_url,
            token,
            shift_id,
            payload: Box::new(payload),
        });
    }

    pub fn update_assignment(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: ApiAssignmentUpdate,
    ) {
        let _ = self.commands.send(RemoteCommand::UpdateAssignment {
            request_id,
            base_url,
            token,
            id,
            payload: Box::new(payload),
        });
    }

    pub fn delete_assignment(&self, request_id: u64, base_url: String, token: String, id: String) {
        let _ = self.commands.send(RemoteCommand::DeleteAssignment {
            request_id,
            base_url,
            token,
            id,
        });
    }

    pub fn shift_suitability(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        shift_id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::ShiftSuitability {
            request_id,
            base_url,
            token,
            shift_id,
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
            RemoteCommand::ListRelationships {
                request_id,
                base_url,
                token,
                contact_id,
                organisation_id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.list_relationships(
                            &token,
                            contact_id.as_deref(),
                            organisation_id.as_deref(),
                        )
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::RelationshipsListed {
                    request_id,
                    contact_id,
                    organisation_id,
                    result,
                });
            }
            RemoteCommand::RelationshipGraph {
                request_id,
                base_url,
                token,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.relationship_graph(&token))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::RelationshipGraphLoaded { request_id, result });
            }
            RemoteCommand::CreateRelationship {
                request_id,
                base_url,
                token,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_relationship(&token, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::RelationshipCreated { request_id, result });
            }
            RemoteCommand::DeleteRelationship {
                request_id,
                base_url,
                token,
                id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.delete_relationship(&token, &id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::RelationshipDeleted {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::ListSites {
                request_id,
                base_url,
                token,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.list_sites(&token))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::SitesListed { request_id, result });
            }
            RemoteCommand::CreateSite {
                request_id,
                base_url,
                token,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_site(&token, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::SiteCreated { request_id, result });
            }
            RemoteCommand::ListShifts {
                request_id,
                base_url,
                token,
                from,
                to,
                status,
                site_id,
                division_id,
                service_id,
                cost_centre_id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.list_shifts(
                            &token,
                            from.as_deref(),
                            to.as_deref(),
                            status.as_deref(),
                            site_id.as_deref(),
                            division_id.as_deref(),
                            service_id.as_deref(),
                            cost_centre_id.as_deref(),
                        )
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ShiftsListed { request_id, result });
            }
            RemoteCommand::RosterFilters {
                request_id,
                base_url,
                token,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        let divisions = client.list_divisions(&token)?;
                        let services = client.list_services(&token)?;
                        let cost_centres = client.list_cost_centres(&token)?;
                        Ok((divisions, services, cost_centres))
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::RosterFiltersLoaded { request_id, result });
            }
            RemoteCommand::CreateShifts {
                request_id,
                base_url,
                token,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_shifts(&token, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ShiftsCreated { request_id, result });
            }
            RemoteCommand::CopyShift {
                request_id,
                base_url,
                token,
                id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.copy_shift(&token, &id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ShiftCopied { request_id, result });
            }
            RemoteCommand::DeleteShift {
                request_id,
                base_url,
                token,
                id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.delete_shift(&token, &id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ShiftDeleted {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::GetShift {
                request_id,
                base_url,
                token,
                id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.get_shift(&token, &id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ShiftLoaded {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::CreateShift {
                request_id,
                base_url,
                token,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_shift(&token, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ShiftCreated { request_id, result });
            }
            RemoteCommand::UpdateShift {
                request_id,
                base_url,
                token,
                id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.update_shift(&token, &id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ShiftUpdated {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::CreateAssignment {
                request_id,
                base_url,
                token,
                shift_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_assignment(&token, &shift_id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::AssignmentCreated {
                    request_id,
                    shift_id,
                    result,
                });
            }
            RemoteCommand::UpdateAssignment {
                request_id,
                base_url,
                token,
                id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.update_assignment(&token, &id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::AssignmentUpdated {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::DeleteAssignment {
                request_id,
                base_url,
                token,
                id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.delete_assignment(&token, &id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::AssignmentDeleted {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::ShiftSuitability {
                request_id,
                base_url,
                token,
                shift_id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.shift_suitability(&token, &shift_id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::SuitabilityLoaded {
                    request_id,
                    shift_id,
                    result,
                });
            }
        }

        ctx.request_repaint();
    }
}
