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
        ApiAgreement, ApiAgreementFinalised, ApiAgreementLine, ApiAgreementLineWrite,
        ApiAgreementWrite, ApiClientRequirement, ApiClientRequirementWrite, ApiContactDetail,
        ApiContactList, ApiContactWrite, ApiFundingPackage, ApiFundingPackageWrite,
        ApiRelationship, ApiRelationshipEvent, ApiRelationshipGraph, ApiRelationshipList,
        ApiRelationshipType, ApiRelationshipWrite, ApiWorkerQualityValue,
        ApiWorkerQualityValueWrite, RelationshipChange,
    },
    intake_board::{
        ApiChecklistToggleResponse, ApiChecklistToggleWrite, ApiContactMatchList, ApiIntakeBoard,
        ApiIntakeCancelWrite, ApiIntakeCaseCreate, ApiIntakeCaseCreateResponse,
        ApiIntakeCaseDetail, ApiIntakeCaseUpdate, ApiIntakeFieldCatalog, ApiIntakeFlowList,
        ApiTransitionWrite,
    },
    organisation::{ApiOrganisation, ApiOrganisationList, ApiOrganisationWrite},
    rostering::{
        ApiAssignment, ApiAssignmentUpdate, ApiAssignmentWrite, ApiBatchShiftWrite,
        ApiCopyShiftWrite, ApiCostCentre, ApiDivision, ApiService, ApiShift, ApiShiftDetail,
        ApiShiftList, ApiShiftUpdate, ApiShiftWrite, ApiSite, ApiSiteWrite, ApiSuitability,
    },
    store::{ContactCursor, ContactFilter},
};

pub enum RemoteCommand {
    LoadIntakeFlows {
        request_id: u64,
        base_url: String,
        token: String,
    },
    LoadIntakeFields {
        request_id: u64,
        base_url: String,
        token: String,
    },
    MatchIntakeContacts {
        request_id: u64,
        base_url: String,
        token: String,
        search: String,
    },
    CreateIntakeCase {
        request_id: u64,
        base_url: String,
        token: String,
        payload: Box<ApiIntakeCaseCreate>,
    },
    UpdateIntakeCase {
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
        payload: Box<ApiIntakeCaseUpdate>,
    },
    ToggleIntakeChecklist {
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
        item_id: String,
        payload: Box<ApiChecklistToggleWrite>,
    },
    CancelIntakeCase {
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
        payload: Box<ApiIntakeCancelWrite>,
    },
    LoadIntakeBoard {
        request_id: u64,
        base_url: String,
        token: String,
        flow_ids: Vec<String>,
        subject_kind: Option<String>,
        assignee: String,
        search: String,
    },
    LoadIntakeCase {
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
    },
    TransitionIntakeCase {
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
        payload: Box<ApiTransitionWrite>,
    },
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
    SetContactQuality {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        payload: Box<ApiWorkerQualityValueWrite>,
    },
    DeleteContactQuality {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        quality_id: String,
    },
    SetContactRequirement {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        payload: Box<ApiClientRequirementWrite>,
    },
    DeleteContactRequirement {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        requirement_id: String,
    },
    CreateFundingPackage {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        payload: Box<ApiFundingPackageWrite>,
    },
    CreateAgreement {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        payload: Box<ApiAgreementWrite>,
    },
    AddAgreementLine {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        agreement_id: String,
        payload: Box<ApiAgreementLineWrite>,
    },
    DeleteAgreementLine {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        agreement_id: String,
        line_id: String,
    },
    FinaliseAgreement {
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        agreement_id: String,
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
    ChangeRelationship {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        change: Box<RelationshipChange>,
    },
    RelationshipHistory {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    },
    ListRelationshipTypes {
        request_id: u64,
        base_url: String,
        token: String,
    },
    ListOrganisations {
        request_id: u64,
        base_url: String,
        token: String,
        search: String,
        status: &'static str,
        limit: usize,
        offset: usize,
    },
    CreateOrganisation {
        request_id: u64,
        base_url: String,
        token: String,
        payload: Box<ApiOrganisationWrite>,
    },
    UpdateOrganisation {
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: Box<ApiOrganisationWrite>,
    },
    DeleteOrganisation {
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
    IntakeFlowsLoaded {
        request_id: u64,
        result: Result<ApiIntakeFlowList, String>,
    },
    IntakeFieldsLoaded {
        request_id: u64,
        result: Result<ApiIntakeFieldCatalog, String>,
    },
    IntakeContactsMatched {
        request_id: u64,
        result: Result<ApiContactMatchList, String>,
    },
    IntakeCaseCreated {
        request_id: u64,
        result: Result<ApiIntakeCaseCreateResponse, String>,
    },
    IntakeCaseUpdated {
        request_id: u64,
        case_id: String,
        result: Result<ApiIntakeCaseDetail, String>,
    },
    IntakeChecklistToggled {
        request_id: u64,
        case_id: String,
        result: Result<ApiChecklistToggleResponse, String>,
    },
    IntakeCaseCancelled {
        request_id: u64,
        case_id: String,
        result: Result<ApiIntakeCaseDetail, String>,
    },
    IntakeBoardLoaded {
        request_id: u64,
        result: Result<ApiIntakeBoard, String>,
    },
    IntakeCaseLoaded {
        request_id: u64,
        case_id: String,
        result: Result<ApiIntakeCaseDetail, String>,
    },
    IntakeCaseTransitioned {
        request_id: u64,
        case_id: String,
        result: Result<ApiIntakeCaseDetail, String>,
    },
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
    ContactQualitySaved {
        request_id: u64,
        contact_id: String,
        result: Result<ApiWorkerQualityValue, String>,
    },
    ContactQualityDeleted {
        request_id: u64,
        contact_id: String,
        quality_id: String,
        result: Result<(), String>,
    },
    ContactRequirementSaved {
        request_id: u64,
        contact_id: String,
        result: Result<ApiClientRequirement, String>,
    },
    ContactRequirementDeleted {
        request_id: u64,
        contact_id: String,
        requirement_id: String,
        result: Result<(), String>,
    },
    FundingPackageSaved {
        request_id: u64,
        contact_id: String,
        result: Result<ApiFundingPackage, String>,
    },
    AgreementSaved {
        request_id: u64,
        contact_id: String,
        result: Result<ApiAgreement, String>,
    },
    AgreementLineSaved {
        request_id: u64,
        contact_id: String,
        agreement_id: String,
        result: Result<ApiAgreementLine, String>,
    },
    AgreementLineDeleted {
        request_id: u64,
        contact_id: String,
        agreement_id: String,
        line_id: String,
        result: Result<(), String>,
    },
    AgreementFinalised {
        request_id: u64,
        contact_id: String,
        result: Result<ApiAgreementFinalised, String>,
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
    RelationshipChanged {
        request_id: u64,
        id: String,
        result: Result<(), String>,
    },
    RelationshipTypesLoaded {
        request_id: u64,
        result: Result<Vec<ApiRelationshipType>, String>,
    },
    RelationshipHistoryLoaded {
        request_id: u64,
        id: String,
        result: Result<Vec<ApiRelationshipEvent>, String>,
    },
    OrganisationsListed {
        request_id: u64,
        result: Result<ApiOrganisationList, String>,
    },
    OrganisationSaved {
        request_id: u64,
        result: Result<ApiOrganisation, String>,
    },
    OrganisationDeleted {
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

    pub fn load_intake_flows(&self, request_id: u64, base_url: String, token: String) {
        let _ = self.commands.send(RemoteCommand::LoadIntakeFlows {
            request_id,
            base_url,
            token,
        });
    }

    pub fn load_intake_fields(&self, request_id: u64, base_url: String, token: String) {
        let _ = self.commands.send(RemoteCommand::LoadIntakeFields {
            request_id,
            base_url,
            token,
        });
    }

    pub fn match_intake_contacts(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        search: String,
    ) {
        let _ = self.commands.send(RemoteCommand::MatchIntakeContacts {
            request_id,
            base_url,
            token,
            search,
        });
    }

    pub fn create_intake_case(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        payload: ApiIntakeCaseCreate,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateIntakeCase {
            request_id,
            base_url,
            token,
            payload: Box::new(payload),
        });
    }

    pub fn update_intake_case(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
        payload: ApiIntakeCaseUpdate,
    ) {
        let _ = self.commands.send(RemoteCommand::UpdateIntakeCase {
            request_id,
            base_url,
            token,
            case_id,
            payload: Box::new(payload),
        });
    }

    pub fn toggle_intake_checklist(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
        item_id: String,
        payload: ApiChecklistToggleWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::ToggleIntakeChecklist {
            request_id,
            base_url,
            token,
            case_id,
            item_id,
            payload: Box::new(payload),
        });
    }

    pub fn cancel_intake_case(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
        payload: ApiIntakeCancelWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CancelIntakeCase {
            request_id,
            base_url,
            token,
            case_id,
            payload: Box::new(payload),
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn load_intake_board(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        flow_ids: Vec<String>,
        subject_kind: Option<String>,
        assignee: String,
        search: String,
    ) {
        let _ = self.commands.send(RemoteCommand::LoadIntakeBoard {
            request_id,
            base_url,
            token,
            flow_ids,
            subject_kind,
            assignee,
            search,
        });
    }

    pub fn load_intake_case(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::LoadIntakeCase {
            request_id,
            base_url,
            token,
            case_id,
        });
    }

    pub fn transition_intake_case(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        case_id: String,
        payload: ApiTransitionWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::TransitionIntakeCase {
            request_id,
            base_url,
            token,
            case_id,
            payload: Box::new(payload),
        });
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

    pub fn set_contact_quality(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        payload: ApiWorkerQualityValueWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::SetContactQuality {
            request_id,
            base_url,
            token,
            contact_id,
            payload: Box::new(payload),
        });
    }

    pub fn delete_contact_quality(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        quality_id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::DeleteContactQuality {
            request_id,
            base_url,
            token,
            contact_id,
            quality_id,
        });
    }

    pub fn set_contact_requirement(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        payload: ApiClientRequirementWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::SetContactRequirement {
            request_id,
            base_url,
            token,
            contact_id,
            payload: Box::new(payload),
        });
    }

    pub fn delete_contact_requirement(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        requirement_id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::DeleteContactRequirement {
            request_id,
            base_url,
            token,
            contact_id,
            requirement_id,
        });
    }

    pub fn create_funding_package(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        payload: ApiFundingPackageWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateFundingPackage {
            request_id,
            base_url,
            token,
            contact_id,
            payload: Box::new(payload),
        });
    }

    pub fn create_agreement(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        payload: ApiAgreementWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateAgreement {
            request_id,
            base_url,
            token,
            contact_id,
            payload: Box::new(payload),
        });
    }

    pub fn add_agreement_line(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        agreement_id: String,
        payload: ApiAgreementLineWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::AddAgreementLine {
            request_id,
            base_url,
            token,
            contact_id,
            agreement_id,
            payload: Box::new(payload),
        });
    }

    pub fn delete_agreement_line(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        agreement_id: String,
        line_id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::DeleteAgreementLine {
            request_id,
            base_url,
            token,
            contact_id,
            agreement_id,
            line_id,
        });
    }

    pub fn finalise_agreement(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        contact_id: String,
        agreement_id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::FinaliseAgreement {
            request_id,
            base_url,
            token,
            contact_id,
            agreement_id,
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

    pub fn change_relationship(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        change: RelationshipChange,
    ) {
        let _ = self.commands.send(RemoteCommand::ChangeRelationship {
            request_id,
            base_url,
            token,
            id,
            change: Box::new(change),
        });
    }

    pub fn relationship_types(&self, request_id: u64, base_url: String, token: String) {
        let _ = self.commands.send(RemoteCommand::ListRelationshipTypes {
            request_id,
            base_url,
            token,
        });
    }

    pub fn relationship_history(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::RelationshipHistory {
            request_id,
            base_url,
            token,
            id,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn list_organisations(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        search: String,
        status: &'static str,
        limit: usize,
        offset: usize,
    ) {
        let _ = self.commands.send(RemoteCommand::ListOrganisations {
            request_id,
            base_url,
            token,
            search,
            status,
            limit,
            offset,
        });
    }

    pub fn create_organisation(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        payload: ApiOrganisationWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::CreateOrganisation {
            request_id,
            base_url,
            token,
            payload: Box::new(payload),
        });
    }

    pub fn update_organisation(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
        payload: ApiOrganisationWrite,
    ) {
        let _ = self.commands.send(RemoteCommand::UpdateOrganisation {
            request_id,
            base_url,
            token,
            id,
            payload: Box::new(payload),
        });
    }

    pub fn delete_organisation(
        &self,
        request_id: u64,
        base_url: String,
        token: String,
        id: String,
    ) {
        let _ = self.commands.send(RemoteCommand::DeleteOrganisation {
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
            RemoteCommand::LoadIntakeFlows {
                request_id,
                base_url,
                token,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.list_intake_flows(&token))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeFlowsLoaded { request_id, result });
            }
            RemoteCommand::LoadIntakeFields {
                request_id,
                base_url,
                token,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.intake_field_catalog(&token))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeFieldsLoaded { request_id, result });
            }
            RemoteCommand::MatchIntakeContacts {
                request_id,
                base_url,
                token,
                search,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.intake_contact_matches(&token, &search))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeContactsMatched { request_id, result });
            }
            RemoteCommand::CreateIntakeCase {
                request_id,
                base_url,
                token,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_intake_case(&token, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeCaseCreated { request_id, result });
            }
            RemoteCommand::UpdateIntakeCase {
                request_id,
                base_url,
                token,
                case_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.update_intake_case(&token, &case_id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeCaseUpdated {
                    request_id,
                    case_id,
                    result,
                });
            }
            RemoteCommand::ToggleIntakeChecklist {
                request_id,
                base_url,
                token,
                case_id,
                item_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.toggle_intake_checklist(&token, &case_id, &item_id, &payload)
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeChecklistToggled {
                    request_id,
                    case_id,
                    result,
                });
            }
            RemoteCommand::CancelIntakeCase {
                request_id,
                base_url,
                token,
                case_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.cancel_intake_case(&token, &case_id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeCaseCancelled {
                    request_id,
                    case_id,
                    result,
                });
            }
            RemoteCommand::LoadIntakeBoard {
                request_id,
                base_url,
                token,
                flow_ids,
                subject_kind,
                assignee,
                search,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.get_intake_board(
                            &token,
                            &flow_ids,
                            subject_kind.as_deref(),
                            &assignee,
                            &search,
                        )
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeBoardLoaded { request_id, result });
            }
            RemoteCommand::LoadIntakeCase {
                request_id,
                base_url,
                token,
                case_id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.get_intake_case(&token, &case_id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeCaseLoaded {
                    request_id,
                    case_id,
                    result,
                });
            }
            RemoteCommand::TransitionIntakeCase {
                request_id,
                base_url,
                token,
                case_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.transition_intake_case(&token, &case_id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::IntakeCaseTransitioned {
                    request_id,
                    case_id,
                    result,
                });
            }
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
            RemoteCommand::SetContactQuality {
                request_id,
                base_url,
                token,
                contact_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.set_contact_quality(&token, &contact_id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ContactQualitySaved {
                    request_id,
                    contact_id,
                    result,
                });
            }
            RemoteCommand::DeleteContactQuality {
                request_id,
                base_url,
                token,
                contact_id,
                quality_id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.delete_contact_quality(&token, &contact_id, &quality_id)
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ContactQualityDeleted {
                    request_id,
                    contact_id,
                    quality_id,
                    result,
                });
            }
            RemoteCommand::SetContactRequirement {
                request_id,
                base_url,
                token,
                contact_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.set_contact_requirement(&token, &contact_id, &payload)
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ContactRequirementSaved {
                    request_id,
                    contact_id,
                    result,
                });
            }
            RemoteCommand::DeleteContactRequirement {
                request_id,
                base_url,
                token,
                contact_id,
                requirement_id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.delete_contact_requirement(&token, &contact_id, &requirement_id)
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::ContactRequirementDeleted {
                    request_id,
                    contact_id,
                    requirement_id,
                    result,
                });
            }
            RemoteCommand::CreateFundingPackage {
                request_id,
                base_url,
                token,
                contact_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_funding_package(&token, &contact_id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::FundingPackageSaved {
                    request_id,
                    contact_id,
                    result,
                });
            }
            RemoteCommand::CreateAgreement {
                request_id,
                base_url,
                token,
                contact_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_agreement(&token, &contact_id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::AgreementSaved {
                    request_id,
                    contact_id,
                    result,
                });
            }
            RemoteCommand::AddAgreementLine {
                request_id,
                base_url,
                token,
                contact_id,
                agreement_id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.add_agreement_line(&token, &agreement_id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::AgreementLineSaved {
                    request_id,
                    contact_id,
                    agreement_id,
                    result,
                });
            }
            RemoteCommand::DeleteAgreementLine {
                request_id,
                base_url,
                token,
                contact_id,
                agreement_id,
                line_id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.delete_agreement_line(&token, &agreement_id, &line_id)
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::AgreementLineDeleted {
                    request_id,
                    contact_id,
                    agreement_id,
                    line_id,
                    result,
                });
            }
            RemoteCommand::FinaliseAgreement {
                request_id,
                base_url,
                token,
                contact_id,
                agreement_id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.finalise_agreement(&token, &agreement_id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::AgreementFinalised {
                    request_id,
                    contact_id,
                    result,
                });
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
            RemoteCommand::ChangeRelationship {
                request_id,
                base_url,
                token,
                id,
                change,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.change_relationship(&token, &id, &change))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::RelationshipChanged {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::ListRelationshipTypes {
                request_id,
                base_url,
                token,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.relationship_types(&token))
                    .map(|list| list.items)
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::RelationshipTypesLoaded { request_id, result });
            }
            RemoteCommand::RelationshipHistory {
                request_id,
                base_url,
                token,
                id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.relationship_history(&token, &id))
                    .map(|history| history.items)
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::RelationshipHistoryLoaded {
                    request_id,
                    id,
                    result,
                });
            }
            RemoteCommand::ListOrganisations {
                request_id,
                base_url,
                token,
                search,
                status,
                limit,
                offset,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| {
                        client.list_organisations(&token, &search, status, limit, offset)
                    })
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::OrganisationsListed { request_id, result });
            }
            RemoteCommand::CreateOrganisation {
                request_id,
                base_url,
                token,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.create_organisation(&token, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::OrganisationSaved { request_id, result });
            }
            RemoteCommand::UpdateOrganisation {
                request_id,
                base_url,
                token,
                id,
                payload,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.update_organisation(&token, &id, &payload))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::OrganisationSaved { request_id, result });
            }
            RemoteCommand::DeleteOrganisation {
                request_id,
                base_url,
                token,
                id,
            } => {
                let result = ApiClient::new(&base_url)
                    .and_then(|client| client.delete_organisation(&token, &id))
                    .map_err(|err| err.to_string());
                let _ = events.send(RemoteEvent::OrganisationDeleted {
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
