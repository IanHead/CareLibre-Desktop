//! Rostering: a resource-grouped timeline, shifts, assignments and explainable
//! worker suitability from the server's rostering graph.

use std::collections::BTreeMap;

use chrono::{Duration as ChronoDuration, Local, NaiveDate, TimeZone};
use eframe::egui::{
    self, Align, Color32, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke,
    Ui, vec2,
};

use crate::{
    contact::{ApiGraphNode, PartyKind},
    rostering::{
        ApiAssignment, ApiAssignmentUpdate, ApiCopyShiftWrite, ApiCostCentre, ApiDivision,
        ApiService, ApiShift, ApiShiftDetail, ApiShiftUpdate, ApiShiftWrite, ApiSite, ApiSiteWrite,
        ApiSuitability, ApiSuitabilityCandidate, ApiSuitabilityRequirement, add_days, local_date,
        local_inputs, local_range, local_time, local_to_utc, monday_of, parse_local_date,
        parse_utc, shift_kind_label, status_label,
    },
    theme,
};

const TIMELINE_DAYS: i64 = 7;
const LABEL_WIDTH: f32 = 150.0;
const HEADER_HEIGHT: f32 = 34.0;
const BLOCK_HEIGHT: f32 = 24.0;
const BLOCK_GAP: f32 = 4.0;

pub struct RosteringState {
    pub sites: Vec<ApiSite>,
    pub divisions: Vec<ApiDivision>,
    pub services: Vec<ApiService>,
    pub cost_centres: Vec<ApiCostCentre>,
    pub shifts: Vec<ApiShift>,
    pub detail: Option<ApiShiftDetail>,
    pub suitability: Option<ApiSuitability>,
    pub selected: Option<String>,
    pub loading_shifts: bool,
    pub loading_detail: bool,
    pub loading_suitability: bool,
    pub error: Option<String>,
    pub week_start: String,
    pub group_by: String,
    pub filter_status: String,
    pub filter_site: Option<String>,
    pub filter_division: Option<String>,
    pub filter_service: Option<String>,
    pub filter_cost_centre: Option<String>,
    pub requested: bool,
    pub editor: Option<ShiftEditor>,
    pub site_form: Option<SiteForm>,
    pub assignment_role: String,
    pub assignment_target: Option<ApiGraphNode>,
    pub assignment_error: Option<String>,
    pub suitability_error: Option<String>,
    pub assignment_pending: bool,
    pub clipboard: Option<ApiShift>,
    pub paste: Option<PasteForm>,
    pub delete_confirm: Option<String>,
}

impl Default for RosteringState {
    fn default() -> Self {
        let today = Local::now().date_naive();
        Self {
            sites: Vec::new(),
            divisions: Vec::new(),
            services: Vec::new(),
            cost_centres: Vec::new(),
            shifts: Vec::new(),
            detail: None,
            suitability: None,
            selected: None,
            loading_shifts: false,
            loading_detail: false,
            loading_suitability: false,
            error: None,
            week_start: monday_of(today).format("%Y-%m-%d").to_string(),
            group_by: "worker".to_owned(),
            filter_status: "all".to_owned(),
            filter_site: None,
            filter_division: None,
            filter_service: None,
            filter_cost_centre: None,
            requested: false,
            editor: None,
            site_form: None,
            assignment_role: "worker".to_owned(),
            assignment_target: None,
            assignment_error: None,
            suitability_error: None,
            assignment_pending: false,
            clipboard: None,
            paste: None,
            delete_confirm: None,
        }
    }
}

impl RosteringState {
    pub fn week_days(&self) -> Vec<NaiveDate> {
        let start = parse_local_date(&self.week_start);
        (0..TIMELINE_DAYS)
            .map(|day| start + ChronoDuration::days(day))
            .collect()
    }

    fn shift_week(&mut self, delta: i64) {
        self.week_start = add_days(&self.week_start, delta * TIMELINE_DAYS);
    }

    fn open_paste(&mut self) {
        if let Some(source) = &self.clipboard {
            self.paste = Some(PasteForm::new(source));
        }
    }
}

pub struct ShiftEditor {
    pub editing_id: Option<String>,
    pub closed: bool,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub date: String,
    pub start: String,
    pub end: String,
    pub site_id: Option<String>,
    pub location: String,
    pub notes: String,
    pub recurrence: String,
    pub recurrence_count: String,
    pub recurrence_until: String,
    pub error: Option<String>,
    pub saving: bool,
}

impl ShiftEditor {
    pub fn new_for(date: String) -> Self {
        Self {
            editing_id: None,
            closed: false,
            title: String::new(),
            kind: "service_delivery".to_owned(),
            status: "draft".to_owned(),
            date,
            start: "09:00".to_owned(),
            end: "11:00".to_owned(),
            site_id: None,
            location: String::new(),
            notes: String::new(),
            recurrence: "none".to_owned(),
            recurrence_count: "4".to_owned(),
            recurrence_until: String::new(),
            error: None,
            saving: false,
        }
    }

    pub fn from_shift(shift: &ApiShift) -> Self {
        let (date, start) = local_inputs(&shift.starts_at);
        let (_, end) = local_inputs(&shift.ends_at);
        Self {
            editing_id: Some(shift.id.clone()),
            closed: false,
            title: shift.title.clone().unwrap_or_default(),
            kind: shift.kind.clone(),
            status: shift.status.clone(),
            date,
            start,
            end,
            site_id: shift.site_id.clone(),
            location: shift.location.clone().unwrap_or_default(),
            notes: String::new(),
            recurrence: "none".to_owned(),
            recurrence_count: "4".to_owned(),
            recurrence_until: String::new(),
            error: None,
            saving: false,
        }
    }

    fn base_times(&self) -> Result<(String, String), String> {
        let starts_at = local_to_utc(&self.date, &self.start)?;
        let ends_at = local_to_utc(&self.date, &self.end)?;
        if let (Some(start), Some(end)) = (parse_utc(&starts_at), parse_utc(&ends_at))
            && end <= start
        {
            return Err("End time must be after the start time.".to_owned());
        }
        Ok((starts_at, ends_at))
    }

    fn write_for(&self, starts_at: String, ends_at: String) -> ApiShiftWrite {
        ApiShiftWrite {
            title: trim_opt(&self.title),
            kind: Some(self.kind.clone()),
            starts_at,
            ends_at,
            location: trim_opt(&self.location),
            status: Some(self.status.clone()),
            division_id: None,
            service_id: None,
            site_id: self.site_id.clone(),
            notes: trim_opt(&self.notes),
        }
    }

    /// Build every occurrence for a new shift. Recurrence dates are computed in
    /// local wall time and submitted as one atomic batch.
    pub fn build_batch(&self) -> Result<(Vec<ApiShiftWrite>, String), String> {
        let (base_start, base_end) = self.base_times()?;
        if self.recurrence == "none" {
            return Ok((vec![self.write_for(base_start, base_end)], String::new()));
        }

        let step = match self.recurrence.as_str() {
            "daily" => 1,
            "weekly" => 7,
            "fortnightly" => 14,
            other => return Err(format!("Unknown recurrence “{other}”.")),
        };
        let until = if self.recurrence_until.trim().is_empty() {
            None
        } else {
            Some(parse_local_date(&self.recurrence_until))
        };
        let count = self
            .recurrence_count
            .trim()
            .parse::<usize>()
            .unwrap_or(0)
            .clamp(1, 100);
        if until.is_none() && count < 2 {
            return Err(
                "Set at least two occurrences, or an “until” date, for a repeating shift."
                    .to_owned(),
            );
        }

        let start_date = parse_local_date(&self.date);
        let mut dates = Vec::new();
        for index in 0..100usize {
            let date = start_date + ChronoDuration::days((index as i64) * step);
            if let Some(until) = until
                && date > until
            {
                break;
            }
            if until.is_none() && index >= count {
                break;
            }
            dates.push(date);
        }
        if dates.is_empty() {
            return Err("The recurrence produces no shifts.".to_owned());
        }

        let mut shifts = Vec::with_capacity(dates.len());
        for date in dates {
            let date = date.format("%Y-%m-%d").to_string();
            let starts_at = local_to_utc(&date, &self.start)?;
            let ends_at = local_to_utc(&date, &self.end)?;
            shifts.push(self.write_for(starts_at, ends_at));
        }
        Ok((shifts, self.recurrence.clone()))
    }
}

pub struct PasteForm {
    pub source: ApiShift,
    pub date: String,
    pub start: String,
    pub end: String,
    pub include_assignments: bool,
    pub error: Option<String>,
    pub saving: bool,
    pub closed: bool,
}

impl PasteForm {
    fn new(source: &ApiShift) -> Self {
        let (_, start) = local_inputs(&source.starts_at);
        let (_, end) = local_inputs(&source.ends_at);
        Self {
            source: source.clone(),
            date: Local::now().format("%Y-%m-%d").to_string(),
            start,
            end,
            include_assignments: true,
            error: None,
            saving: false,
            closed: false,
        }
    }

    fn build(&self) -> Result<ApiCopyShiftWrite, String> {
        let starts_at = local_to_utc(&self.date, &self.start)?;
        let ends_at = local_to_utc(&self.date, &self.end)?;
        if let (Some(start), Some(end)) = (parse_utc(&starts_at), parse_utc(&ends_at))
            && end <= start
        {
            return Err("End time must be after the start time.".to_owned());
        }
        Ok(ApiCopyShiftWrite {
            starts_at: Some(starts_at),
            ends_at: Some(ends_at),
            include_assignments: self.include_assignments,
        })
    }
}

#[derive(Default)]
pub struct SiteForm {
    pub name: String,
    pub address: String,
    pub error: Option<String>,
    pub saving: bool,
    pub closed: bool,
}

pub enum RosteringAction {
    OpenCreate(String),
    Refresh,
    LoadShift(String),
    CreateShifts {
        shifts: Vec<ApiShiftWrite>,
        recurrence: String,
    },
    UpdateShift {
        id: String,
        payload: ApiShiftUpdate,
    },
    CopyShift {
        id: String,
        payload: ApiCopyShiftWrite,
    },
    DeleteShift(String),
    CreateSite(ApiSiteWrite),
    CreateAssignment {
        shift_id: String,
        contact_id: String,
        role: String,
    },
    UpdateAssignment {
        id: String,
        payload: ApiAssignmentUpdate,
    },
    DeleteAssignment(String),
    FindWorkers(String),
    OpenContact(String),
}

pub fn show(
    ui: &mut Ui,
    state: &mut RosteringState,
    catalog: &[ApiGraphNode],
) -> Option<RosteringAction> {
    let mut action = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            header(ui, state, &mut action);
            ui.add_space(14.0);
            filters(ui, state, &mut action);
            ui.add_space(10.0);

            let mut close_editor = false;
            if let Some(editor) = &mut state.editor {
                editor_card(ui, editor, &state.sites, &mut action);
                close_editor = editor.closed;
                ui.add_space(10.0);
            }
            if close_editor {
                state.editor = None;
            }

            let mut close_paste = false;
            if let Some(paste) = &mut state.paste {
                paste_card(ui, paste, &mut action);
                close_paste = paste.closed;
                ui.add_space(10.0);
            }
            if close_paste {
                state.paste = None;
            }

            let mut close_site = false;
            if let Some(site_form) = &mut state.site_form {
                site_card(ui, site_form, &mut action);
                close_site = site_form.closed;
                ui.add_space(10.0);
            }
            if close_site {
                state.site_form = None;
            }

            if let Some(error) = &state.error {
                error_box(ui, error);
                ui.add_space(8.0);
            }

            timeline_card(ui, state, &mut action);
            ui.add_space(12.0);
            detail_card(ui, state, catalog, &mut action);
            ui.add_space(24.0);
        });

    action
}

fn header(ui: &mut Ui, state: &mut RosteringState, action: &mut Option<RosteringAction>) {
    if ui.available_width() < 700.0 {
        header_title(ui);
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| header_actions(ui, state, action));
    } else {
        ui.horizontal(|ui| {
            header_title(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                header_actions(ui, state, action);
            });
        });
    }
}

fn header_title(ui: &mut Ui) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(52.0, 52.0), Sense::hover());
        ui.painter()
            .rect_filled(rect, CornerRadius::same(14), theme::surface_raised());
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "📅",
            egui::FontId::proportional(22.0),
            theme::text(),
        );
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(RichText::new("Rostering").font(theme::strong_font(26.0)));
            ui.label(
                RichText::new("Timeline by worker or client, with explainable suitability.")
                    .color(theme::muted())
                    .size(13.0),
            );
        });
    });
}

fn header_actions(ui: &mut Ui, state: &mut RosteringState, action: &mut Option<RosteringAction>) {
    if ui.add(theme::primary_button("+ New shift")).clicked() {
        let date = Local::now().format("%Y-%m-%d").to_string();
        *action = Some(RosteringAction::OpenCreate(date));
    }
    if state.clipboard.is_some() && ui.add(theme::secondary_button("Paste shift")).clicked() {
        state.open_paste();
    }
    if ui.add(theme::secondary_button("Refresh")).clicked() {
        *action = Some(RosteringAction::Refresh);
    }
    if state.loading_shifts {
        ui.label(RichText::new("Loading…").color(theme::warning()).size(11.0));
    }
}

fn filters(ui: &mut Ui, state: &mut RosteringState, action: &mut Option<RosteringAction>) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        let before = (
            state.filter_status.clone(),
            state.filter_site.clone(),
            state.filter_division.clone(),
            state.filter_service.clone(),
            state.filter_cost_centre.clone(),
        );

        ui.horizontal_wrapped(|ui| {
            if ui.add(theme::secondary_button("‹")).clicked() {
                state.shift_week(-1);
                *action = Some(RosteringAction::Refresh);
            }
            if ui.add(theme::secondary_button("Today")).clicked() {
                state.week_start = monday_of(Local::now().date_naive())
                    .format("%Y-%m-%d")
                    .to_string();
                *action = Some(RosteringAction::Refresh);
            }
            if ui.add(theme::secondary_button("›")).clicked() {
                state.shift_week(1);
                *action = Some(RosteringAction::Refresh);
            }
            ui.label(
                RichText::new(week_label(&state.week_start))
                    .color(theme::text())
                    .font(theme::strong_font(12.5)),
            );
            ui.separator();
            ui.label(RichText::new("Group by").color(theme::muted()).size(10.5));
            let group = egui::ComboBox::from_id_salt("roster-group")
                .selected_text(if state.group_by == "client" {
                    "Client"
                } else {
                    "Worker"
                })
                .width(100.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.group_by, "worker".to_owned(), "Worker");
                    ui.selectable_value(&mut state.group_by, "client".to_owned(), "Client");
                });
            name_combo(&group.response, "Group timeline by");
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            combo_filter(
                ui,
                "Status",
                "roster-status",
                &mut state.filter_status,
                "all",
                |ui, value| {
                    for status in ["all", "draft", "published", "completed", "cancelled"] {
                        let label = if status == "all" {
                            "All statuses".to_owned()
                        } else {
                            status_label(status)
                        };
                        ui.selectable_value(value, status.to_owned(), label);
                    }
                },
            );
            site_filter(ui, state);
            division_filter(ui, state);
            service_filter(ui, state);
            cost_centre_filter(ui, state);

            if ui
                .add(
                    egui::Button::new(
                        RichText::new("+ Add site")
                            .color(theme::accent())
                            .size(10.5),
                    )
                    .frame(false),
                )
                .clicked()
            {
                state.site_form = Some(SiteForm::default());
            }
            ui.label(
                RichText::new("Filters apply immediately")
                    .color(theme::faint())
                    .size(9.5),
            );
        });

        let after = (
            state.filter_status.clone(),
            state.filter_site.clone(),
            state.filter_division.clone(),
            state.filter_service.clone(),
            state.filter_cost_centre.clone(),
        );
        if before != after {
            *action = Some(RosteringAction::Refresh);
        }
    });
}

fn combo_filter(
    ui: &mut Ui,
    label: &str,
    id: &str,
    value: &mut String,
    selected: &str,
    contents: impl FnOnce(&mut Ui, &mut String),
) {
    ui.label(RichText::new(label).color(theme::muted()).size(10.5));
    let mut current = value.clone();
    let combo = egui::ComboBox::from_id_salt(id)
        .selected_text(if selected == "all" {
            "All".to_owned()
        } else {
            selected.to_owned()
        })
        .width(120.0)
        .show_ui(ui, |ui| {
            contents(ui, &mut current);
        });
    name_combo(&combo.response, label);
    if current != *value {
        *value = current;
    }
}

fn site_filter(ui: &mut Ui, state: &mut RosteringState) {
    ui.label(RichText::new("Site").color(theme::muted()).size(10.5));
    let label = state
        .filter_site
        .as_ref()
        .and_then(|id| state.sites.iter().find(|site| &site.id == id))
        .map(|site| site.name.clone())
        .unwrap_or_else(|| "All sites".to_owned());
    let combo = egui::ComboBox::from_id_salt("roster-site-filter")
        .selected_text(label)
        .width(150.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(state.filter_site.is_none(), "All sites")
                .clicked()
            {
                state.filter_site = None;
            }
            for site in &state.sites {
                let selected = state.filter_site.as_deref() == Some(site.id.as_str());
                if ui.selectable_label(selected, &site.name).clicked() {
                    state.filter_site = Some(site.id.clone());
                }
            }
        });
    name_combo(&combo.response, "Site filter");
}

fn division_filter(ui: &mut Ui, state: &mut RosteringState) {
    ui.label(RichText::new("Division").color(theme::muted()).size(10.5));
    let label = state
        .filter_division
        .as_ref()
        .and_then(|id| state.divisions.iter().find(|division| &division.id == id))
        .map(|division| division.name.clone())
        .unwrap_or_else(|| "All divisions".to_owned());
    let combo = egui::ComboBox::from_id_salt("roster-division-filter")
        .selected_text(label)
        .width(130.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(state.filter_division.is_none(), "All divisions")
                .clicked()
            {
                state.filter_division = None;
            }
            for division in &state.divisions {
                let selected = state.filter_division.as_deref() == Some(division.id.as_str());
                if ui.selectable_label(selected, &division.name).clicked() {
                    state.filter_division = Some(division.id.clone());
                }
            }
        });
    name_combo(&combo.response, "Division filter");
}

fn service_filter(ui: &mut Ui, state: &mut RosteringState) {
    ui.label(RichText::new("Service").color(theme::muted()).size(10.5));
    let label = state
        .filter_service
        .as_ref()
        .and_then(|id| state.services.iter().find(|service| &service.id == id))
        .map(|service| service.name.clone())
        .unwrap_or_else(|| "All services".to_owned());
    let combo = egui::ComboBox::from_id_salt("roster-service-filter")
        .selected_text(label)
        .width(150.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(state.filter_service.is_none(), "All services")
                .clicked()
            {
                state.filter_service = None;
            }
            for service in &state.services {
                let selected = state.filter_service.as_deref() == Some(service.id.as_str());
                if ui.selectable_label(selected, &service.name).clicked() {
                    state.filter_service = Some(service.id.clone());
                }
            }
        });
    name_combo(&combo.response, "Service filter");
}

fn cost_centre_filter(ui: &mut Ui, state: &mut RosteringState) {
    ui.label(
        RichText::new("Cost centre")
            .color(theme::muted())
            .size(10.5),
    );
    let label = state
        .filter_cost_centre
        .as_ref()
        .and_then(|id| state.cost_centres.iter().find(|cc| &cc.id == id))
        .map(|cc| format!("{} · {}", cc.code, cc.name))
        .unwrap_or_else(|| "All cost centres".to_owned());
    let combo = egui::ComboBox::from_id_salt("roster-cost-centre-filter")
        .selected_text(label)
        .width(180.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(state.filter_cost_centre.is_none(), "All cost centres")
                .clicked()
            {
                state.filter_cost_centre = None;
            }
            for cc in &state.cost_centres {
                let selected = state.filter_cost_centre.as_deref() == Some(cc.id.as_str());
                if ui
                    .selectable_label(selected, format!("{} · {}", cc.code, cc.name))
                    .clicked()
                {
                    state.filter_cost_centre = Some(cc.id.clone());
                }
            }
        });
    name_combo(&combo.response, "Cost centre filter");
}

struct Lane {
    name: String,
    contact_id: Option<String>,
    shifts: Vec<(ApiShift, usize)>,
    rows: usize,
}

fn build_lanes(shifts: &[ApiShift], group_by: &str) -> Vec<Lane> {
    let mut map: BTreeMap<String, (Vec<ApiShift>, Option<String>)> = BTreeMap::new();
    for shift in shifts {
        let parties = if group_by == "client" {
            &shift.clients
        } else {
            &shift.workers
        };
        if parties.is_empty() {
            map.entry("Unassigned".to_owned())
                .or_insert_with(|| (Vec::new(), None))
                .0
                .push(shift.clone());
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        for party in parties {
            if seen.insert(party.id.clone()) {
                map.entry(party.display_name.clone())
                    .or_insert_with(|| (Vec::new(), Some(party.id.clone())))
                    .0
                    .push(shift.clone());
            }
        }
    }

    let mut lanes = Vec::new();
    for (name, (mut lane_shifts, contact_id)) in map {
        lane_shifts.sort_by(|a, b| a.starts_at.cmp(&b.starts_at));
        let mut ends: Vec<f64> = Vec::new();
        let mut placed = Vec::new();
        for shift in lane_shifts {
            let start = parse_utc(&shift.starts_at)
                .map(|value| value.timestamp_millis() as f64 / 60_000.0)
                .unwrap_or(0.0);
            let end = parse_utc(&shift.ends_at)
                .map(|value| value.timestamp_millis() as f64 / 60_000.0)
                .unwrap_or(start);
            let row = ends
                .iter()
                .position(|last_end| *last_end <= start)
                .unwrap_or_else(|| {
                    ends.push(0.0);
                    ends.len() - 1
                });
            ends[row] = end;
            placed.push((shift, row));
        }
        let rows = ends.len().max(1);
        lanes.push(Lane {
            name,
            contact_id,
            shifts: placed,
            rows,
        });
    }
    lanes.sort_by(|a, b| {
        (a.name == "Unassigned")
            .cmp(&(b.name == "Unassigned"))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    lanes
}

fn timeline_card(ui: &mut Ui, state: &RosteringState, action: &mut Option<RosteringAction>) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Timeline").font(theme::strong_font(15.0)));
            ui.label(
                RichText::new(week_label(&state.week_start))
                    .color(theme::muted())
                    .size(11.0),
            );
            theme::pill(
                ui,
                &format!("{} shifts", state.shifts.len()),
                theme::accent(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if state.loading_shifts {
                    ui.label(RichText::new("Loading…").color(theme::warning()).size(10.5));
                }
            });
        });
        ui.add_space(6.0);

        if state.shifts.is_empty() {
            ui.label(
                RichText::new("No shifts in this week match the filters.")
                    .color(theme::faint())
                    .size(11.0),
            );
            return;
        }

        let lanes = build_lanes(&state.shifts, &state.group_by);
        let lane_meta: Vec<(String, f32)> = lanes
            .iter()
            .map(|lane| {
                (
                    lane.name.clone(),
                    lane.rows as f32 * (BLOCK_HEIGHT + BLOCK_GAP) + BLOCK_GAP,
                )
            })
            .collect();
        let height = HEADER_HEIGHT
            + lanes
                .iter()
                .map(|lane| lane.rows as f32 * (BLOCK_HEIGHT + BLOCK_GAP) + BLOCK_GAP)
                .sum::<f32>()
            + 4.0;

        let scroll = ScrollArea::vertical()
            .id_salt("roster_timeline")
            .max_height(440.0)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);
        let output = scroll.show(ui, |ui| {
            let width = ui.available_width();
            let day_width = ((width - LABEL_WIDTH) / TIMELINE_DAYS as f32).max(1.0);
            let px_per_hour = day_width / 24.0;
            let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, CornerRadius::same(8), theme::surface());

            let week_start = week_start_local(&state.week_start);

            for day in 0..TIMELINE_DAYS {
                let x = rect.left() + LABEL_WIDTH + day as f32 * day_width;
                painter.line_segment(
                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                    Stroke::new(1.0, theme::border()),
                );
                if day_width >= 30.0 {
                    let date = week_start + ChronoDuration::days(day);
                    painter.text(
                        egui::pos2(x + 6.0, rect.top() + 10.0),
                        egui::Align2::LEFT_TOP,
                        date.format("%a %-d").to_string(),
                        theme::strong_font(10.0),
                        theme::muted(),
                    );
                }
            }

            let show_hour_labels = px_per_hour * 3.0 >= 14.0;
            for hour in (0..=24).step_by(3) {
                let x = rect.left() + LABEL_WIDTH + hour as f32 * px_per_hour;
                painter.line_segment(
                    [
                        egui::pos2(x, rect.top() + HEADER_HEIGHT - 4.0),
                        egui::pos2(x, rect.bottom()),
                    ],
                    Stroke::new(0.5, theme::border().gamma_multiply(0.5)),
                );
                if hour < 24 && show_hour_labels {
                    painter.text(
                        egui::pos2(x + 2.0, rect.top() + HEADER_HEIGHT - 16.0),
                        egui::Align2::LEFT_TOP,
                        format!("{hour:02}"),
                        egui::FontId::proportional(8.5),
                        theme::faint(),
                    );
                }
            }

            let mut y = rect.top() + HEADER_HEIGHT;
            for lane in &lanes {
                let lane_height = lane.rows as f32 * (BLOCK_HEIGHT + BLOCK_GAP) + BLOCK_GAP;
                let lane_rect =
                    egui::Rect::from_min_size(egui::pos2(rect.left(), y), vec2(width, lane_height));
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left(), y),
                        vec2(LABEL_WIDTH, lane_height),
                    ),
                    CornerRadius::ZERO,
                    theme::sidebar(),
                );
                let label_rect = egui::Rect::from_min_size(
                    egui::pos2(rect.left(), y),
                    vec2(LABEL_WIDTH, lane_height),
                );
                let label_response = ui.interact(
                    label_rect,
                    egui::Id::unique(("roster-lane", &lane.name, &lane.contact_id)),
                    Sense::click(),
                );
                let label_colour = if lane.contact_id.is_some() && label_response.hovered() {
                    theme::accent()
                } else {
                    theme::text()
                };
                painter.text(
                    egui::pos2(rect.left() + 8.0, y + 6.0),
                    egui::Align2::LEFT_TOP,
                    &lane.name,
                    theme::strong_font(10.5),
                    label_colour,
                );
                if label_response.clicked()
                    && let Some(contact_id) = &lane.contact_id
                {
                    *action = Some(RosteringAction::OpenContact(contact_id.clone()));
                }

                for (shift, row) in &lane.shifts {
                    let Some(start) = parse_utc(&shift.starts_at) else {
                        continue;
                    };
                    let Some(end) = parse_utc(&shift.ends_at) else {
                        continue;
                    };
                    let offset_minutes =
                        start.signed_duration_since(week_start).num_minutes() as f32;
                    let duration_minutes = end.signed_duration_since(start).num_minutes() as f32;
                    let x = rect.left() + LABEL_WIDTH + (offset_minutes / 60.0) * px_per_hour;
                    let block_width = ((duration_minutes / 60.0) * px_per_hour).max(2.0);
                    let block = egui::Rect::from_min_size(
                        egui::pos2(x, y + BLOCK_GAP + *row as f32 * (BLOCK_HEIGHT + BLOCK_GAP)),
                        vec2(block_width, BLOCK_HEIGHT),
                    );
                    if block.right() < rect.left() + LABEL_WIDTH || block.left() > rect.right() {
                        continue;
                    }

                    let colour = status_colour(&shift.status);
                    painter.rect_filled(
                        block,
                        CornerRadius::same(5),
                        Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), 55),
                    );
                    painter.rect_stroke(
                        block,
                        CornerRadius::same(5),
                        Stroke::new(
                            if state.selected.as_deref() == Some(shift.id.as_str()) {
                                1.8
                            } else {
                                0.8
                            },
                            colour,
                        ),
                        egui::StrokeKind::Inside,
                    );
                    if block_width >= 24.0 {
                        let text_painter = painter.with_clip_rect(block.shrink(4.0));
                        let label = if block_width < 62.0 {
                            local_time(&shift.starts_at)
                        } else {
                            format!(
                                "{} {}",
                                local_time(&shift.starts_at),
                                shift
                                    .title
                                    .clone()
                                    .or_else(|| shift.service_name.clone())
                                    .unwrap_or_else(|| shift_kind_label(&shift.kind))
                            )
                        };
                        text_painter.text(
                            egui::pos2(block.left() + 5.0, block.top() + 3.0),
                            egui::Align2::LEFT_TOP,
                            label,
                            egui::FontId::proportional(if block_width < 62.0 { 8.0 } else { 9.5 }),
                            theme::text(),
                        );
                    }

                    let response = ui
                        .interact(
                            block,
                            egui::Id::unique(("roster-block", &lane.name, &shift.id, row)),
                            Sense::click(),
                        )
                        .on_hover_text(format!(
                            "{}\n{} – {}\n{}{}",
                            shift
                                .title
                                .clone()
                                .or_else(|| shift.service_name.clone())
                                .unwrap_or_else(|| shift_kind_label(&shift.kind)),
                            local_time(&shift.starts_at),
                            local_time(&shift.ends_at),
                            shift
                                .site_name
                                .as_deref()
                                .or(shift.location.as_deref())
                                .unwrap_or("No site"),
                            shift
                                .cost_centre_name
                                .as_ref()
                                .map(|cc| format!("\nCost centre: {cc}"))
                                .unwrap_or_default(),
                        ));
                    if response.clicked() {
                        *action = Some(RosteringAction::LoadShift(shift.id.clone()));
                    }
                    if response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }

                painter.line_segment(
                    [
                        egui::pos2(rect.left(), y + lane_height),
                        egui::pos2(rect.right(), y + lane_height),
                    ],
                    Stroke::new(0.5, theme::border()),
                );
                y += lane_height;
                let _ = lane_rect;
            }
        });

        let offset = output.state.clamped_offset();
        let painter = ui.painter_at(output.inner_rect);
        let mut y = output.inner_rect.top() - offset.y + HEADER_HEIGHT;
        for (name, height) in &lane_meta {
            let label_rect = egui::Rect::from_min_size(
                egui::pos2(output.inner_rect.left(), y),
                vec2(LABEL_WIDTH, *height),
            );
            painter.rect_filled(label_rect, CornerRadius::ZERO, theme::sidebar());
            painter.text(
                egui::pos2(label_rect.left() + 8.0, y + 6.0),
                egui::Align2::LEFT_TOP,
                name,
                theme::strong_font(10.5),
                theme::text(),
            );
            painter.line_segment(
                [
                    egui::pos2(label_rect.left(), y + *height),
                    egui::pos2(label_rect.left() + LABEL_WIDTH, y + *height),
                ],
                Stroke::new(0.5, theme::border()),
            );
            y += *height;
        }

        let header_rect = egui::Rect::from_min_size(
            egui::pos2(output.inner_rect.left(), output.inner_rect.top() - offset.y),
            vec2(LABEL_WIDTH, HEADER_HEIGHT),
        );
        painter.rect_filled(header_rect, CornerRadius::ZERO, theme::sidebar());
        painter.text(
            egui::pos2(header_rect.left() + 8.0, header_rect.top() + 10.0),
            egui::Align2::LEFT_TOP,
            if state.group_by == "client" {
                "Client"
            } else {
                "Worker"
            },
            theme::strong_font(10.0),
            theme::faint(),
        );
    });
}

fn week_start_local(value: &str) -> chrono::DateTime<Local> {
    let date = parse_local_date(value);
    Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap_or_else(Local::now)
}

fn status_colour(status: &str) -> Color32 {
    match status {
        "published" => theme::success(),
        "draft" => theme::warning(),
        "cancelled" => theme::danger(),
        "completed" => theme::accent_blue(),
        _ => theme::muted(),
    }
}

fn detail_card(
    ui: &mut Ui,
    state: &mut RosteringState,
    catalog: &[ApiGraphNode],
    action: &mut Option<RosteringAction>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        let Some(detail) = state.detail.clone() else {
            ui.label(RichText::new("Shift detail").font(theme::strong_font(15.0)));
            ui.label(
                RichText::new("Select a shift in the timeline to see and edit its roster.")
                    .color(theme::faint())
                    .size(11.0),
            );
            return;
        };

        let shift = &detail.shift;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    shift
                        .title
                        .clone()
                        .or_else(|| shift.service_name.clone())
                        .unwrap_or_else(|| shift_kind_label(&shift.kind)),
                )
                .font(theme::strong_font(17.0)),
            );
            status_pill(ui, &shift.status);
            if let Some(recurrence) = &shift.recurrence {
                theme::pill(ui, &format!("↻ {recurrence}"), theme::warning());
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Delete").color(theme::danger()).size(10.5),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    *action = Some(RosteringAction::DeleteShift(shift.id.clone()));
                }
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Copy").color(theme::accent_blue()).size(10.5),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    state.clipboard = Some(shift.clone());
                    state.open_paste();
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new("Edit").color(theme::accent()).size(10.5))
                            .frame(false),
                    )
                    .clicked()
                {
                    state.editor = Some(ShiftEditor::from_shift(shift));
                }
            });
        });

        ui.label(
            RichText::new(format!(
                "{} · {}",
                local_date(&shift.starts_at),
                local_range(&shift.starts_at, &shift.ends_at)
            ))
            .color(theme::muted())
            .size(11.0),
        );
        ui.add_space(4.0);
        meta_row(ui, "Kind", &shift_kind_label(&shift.kind));
        if let Some(site) = &shift.site_name {
            meta_row(ui, "Site", site);
        }
        if let Some(service) = &shift.service_name {
            meta_row(ui, "Service", service);
        }
        if let Some(division) = &shift.division_name {
            meta_row(ui, "Division", division);
        }
        if let Some(cc) = &shift.cost_centre_name {
            meta_row(
                ui,
                "Cost centre",
                &format!(
                    "{}{}",
                    shift
                        .cost_centre_code
                        .as_ref()
                        .map(|code| format!("{code} · "))
                        .unwrap_or_default(),
                    cc
                ),
            );
        }
        if let Some(location) = &shift.location {
            meta_row(ui, "Location", location);
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(RichText::new("Assignments").font(theme::strong_font(13.0)));
        if detail.assignments.is_empty() {
            ui.label(
                RichText::new("No one is assigned yet.")
                    .color(theme::faint())
                    .size(10.5),
            );
        }
        for assignment in &detail.assignments {
            assignment_row(ui, assignment, action);
        }
        ui.add_space(6.0);
        assignment_add(ui, shift, state, catalog, action);

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(8.0);
        suitability_section(ui, state, action);
    });
}

fn assignment_row(ui: &mut Ui, assignment: &ApiAssignment, action: &mut Option<RosteringAction>) {
    Frame::new()
        .fill(theme::surface_raised())
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let role_label = if assignment.role == "worker" {
                    "Worker"
                } else {
                    "Client"
                };
                contact_link(
                    ui,
                    &assignment.contact_name,
                    &assignment.contact_id,
                    action,
                    true,
                );
                theme::pill(ui, role_label, theme::accent_blue());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Remove").color(theme::faint()).size(9.5),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        *action = Some(RosteringAction::DeleteAssignment(assignment.id.clone()));
                    }
                });
            });

            ui.horizontal_wrapped(|ui| {
                let mut status = assignment.status.clone();
                let status_combo =
                    egui::ComboBox::from_id_salt(("assignment-status", &assignment.id))
                        .selected_text(status_label(&status))
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            for option in ["assigned", "attended", "no_show", "cancelled"] {
                                ui.selectable_value(
                                    &mut status,
                                    option.to_owned(),
                                    status_label(option),
                                );
                            }
                        });
                name_combo(&status_combo.response, "Assignment status");
                if status != assignment.status {
                    *action = Some(RosteringAction::UpdateAssignment {
                        id: assignment.id.clone(),
                        payload: ApiAssignmentUpdate {
                            status: Some(status),
                            outcome: None,
                            outcome_rating: None,
                            notes: None,
                        },
                    });
                }

                if assignment.role == "worker" {
                    let mut outcome = assignment.outcome.clone().unwrap_or_default();
                    let outcome_combo =
                        egui::ComboBox::from_id_salt(("assignment-outcome", &assignment.id))
                            .selected_text(if outcome.is_empty() {
                                "Outcome".to_owned()
                            } else {
                                status_label(&outcome)
                            })
                            .width(110.0)
                            .show_ui(ui, |ui| {
                                for option in ["positive", "neutral", "negative"] {
                                    ui.selectable_value(
                                        &mut outcome,
                                        option.to_owned(),
                                        status_label(option),
                                    );
                                }
                            });
                    name_combo(&outcome_combo.response, "Assignment outcome");
                    if outcome != assignment.outcome.clone().unwrap_or_default()
                        && !outcome.is_empty()
                    {
                        *action = Some(RosteringAction::UpdateAssignment {
                            id: assignment.id.clone(),
                            payload: ApiAssignmentUpdate {
                                status: None,
                                outcome: Some(outcome),
                                outcome_rating: None,
                                notes: None,
                            },
                        });
                    }

                    let mut rating = assignment.outcome_rating.unwrap_or(0);
                    let rating_combo =
                        egui::ComboBox::from_id_salt(("assignment-rating", &assignment.id))
                            .selected_text(if rating == 0 {
                                "Rating".to_owned()
                            } else {
                                format!("{rating} / 5")
                            })
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for value in 1..=5 {
                                    ui.selectable_value(&mut rating, value, format!("{value} / 5"));
                                }
                            });
                    name_combo(&rating_combo.response, "Assignment rating");
                    if rating != assignment.outcome_rating.unwrap_or(0) && rating > 0 {
                        *action = Some(RosteringAction::UpdateAssignment {
                            id: assignment.id.clone(),
                            payload: ApiAssignmentUpdate {
                                status: None,
                                outcome: None,
                                outcome_rating: Some(rating),
                                notes: None,
                            },
                        });
                    }
                }
            });
        });
    ui.add_space(4.0);
}

fn assignment_add(
    ui: &mut Ui,
    shift: &ApiShift,
    state: &mut RosteringState,
    catalog: &[ApiGraphNode],
    action: &mut Option<RosteringAction>,
) {
    ui.add_space(4.0);
    ui.label(
        RichText::new("Add assignment")
            .color(theme::muted())
            .font(theme::strong_font(10.5)),
    );
    ui.horizontal_wrapped(|ui| {
        let label = state
            .assignment_target
            .as_ref()
            .map(|node| node.label.clone())
            .unwrap_or_else(|| "Select a contact…".to_owned());
        let contact_combo = egui::ComboBox::from_id_salt("assignment-contact")
            .selected_text(label)
            .width(220.0)
            .show_ui(ui, |ui| {
                let search_id = ui.scope_id().with("assignment-search");
                let mut search =
                    ui.data_mut(|data| data.get_temp::<String>(search_id).unwrap_or_default());
                ui.add(
                    egui::TextEdit::singleline(&mut search)
                        .hint_text("Search contacts…")
                        .desired_width(220.0),
                );
                let needle = search.trim().to_lowercase();
                let mut chosen = None;
                ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    let mut shown = 0;
                    for node in catalog
                        .iter()
                        .filter(|node| node.kind == PartyKind::Contact)
                    {
                        if !needle.is_empty() && !node.label.to_lowercase().contains(&needle) {
                            continue;
                        }
                        if shown >= 50 {
                            break;
                        }
                        shown += 1;
                        let selected = state
                            .assignment_target
                            .as_ref()
                            .is_some_and(|target| target.id == node.id);
                        if ui.selectable_label(selected, &node.label).clicked() {
                            chosen = Some(node.clone());
                            ui.close();
                        }
                    }
                    if shown == 0 {
                        ui.label("No matching contacts");
                    }
                });
                ui.data_mut(|data| data.insert_temp(search_id, search));
                if let Some(node) = chosen {
                    state.assignment_target = Some(node);
                }
            });
        name_combo(&contact_combo.response, "Assignment contact");

        let role_combo = egui::ComboBox::from_id_salt("assignment-role")
            .selected_text(if state.assignment_role == "worker" {
                "Worker"
            } else {
                "Client"
            })
            .width(100.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.assignment_role, "worker".to_owned(), "Worker");
                ui.selectable_value(&mut state.assignment_role, "client".to_owned(), "Client");
            });
        name_combo(&role_combo.response, "Assignment role");

        let ready = state.assignment_target.is_some() && !state.assignment_pending;
        if ui
            .add_enabled(ready, theme::secondary_button("Assign"))
            .clicked()
            && let Some(target) = &state.assignment_target
        {
            state.assignment_pending = true;
            state.assignment_error = None;
            *action = Some(RosteringAction::CreateAssignment {
                shift_id: shift.id.clone(),
                contact_id: target.id.clone(),
                role: state.assignment_role.clone(),
            });
        }
    });
    if let Some(error) = &state.assignment_error {
        ui.label(RichText::new(error).color(theme::danger()).size(10.0));
    }
}

fn suitability_section(
    ui: &mut Ui,
    state: &mut RosteringState,
    action: &mut Option<RosteringAction>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Worker suitability").font(theme::strong_font(13.0)));
        ui.label(
            RichText::new("Advisory — every score explains itself")
                .color(theme::faint())
                .size(9.5),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let shift_id = state.detail.as_ref().map(|detail| detail.shift.id.clone());
            if ui
                .add(theme::secondary_button("Find suitable workers"))
                .clicked()
                && let Some(shift_id) = shift_id
            {
                state.loading_suitability = true;
                state.suitability_error = None;
                *action = Some(RosteringAction::FindWorkers(shift_id));
            }
        });
    });

    if state.loading_suitability {
        ui.label(
            RichText::new("Scoring workers…")
                .color(theme::muted())
                .size(10.5),
        );
    }
    if let Some(error) = &state.suitability_error {
        ui.label(RichText::new(error).color(theme::danger()).size(10.0));
    }

    let Some(suitability) = state.suitability.clone() else {
        return;
    };

    ui.add_space(4.0);
    if !suitability.requirements.is_empty() {
        ui.label(
            RichText::new("Shift requirements")
                .color(theme::text())
                .font(theme::strong_font(10.5)),
        );
        for requirement in &suitability.requirements {
            ui.label(
                RichText::new(format!(
                    "• {} — {}",
                    requirement.client_name,
                    requirement.summary()
                ))
                .color(theme::muted())
                .size(9.5),
            );
        }
        ui.add_space(4.0);
    }

    let (met, unmet): (Vec<_>, Vec<_>) = suitability
        .candidates
        .iter()
        .partition(|candidate| candidate.meets_requirements);
    for candidate in &met {
        suitability_candidate_card(ui, state, action, &suitability.shift_id, candidate);
        ui.add_space(4.0);
    }
    if !unmet.is_empty() {
        ui.add_space(4.0);
        ui.label(
            RichText::new("Does not meet requirements — still assignable")
                .color(theme::warning())
                .font(theme::strong_font(10.5)),
        );
        ui.add_space(2.0);
        for candidate in &unmet {
            suitability_candidate_card(ui, state, action, &suitability.shift_id, candidate);
            ui.add_space(4.0);
        }
    }

    if !suitability.blocked.is_empty() {
        ui.add_space(4.0);
        ui.label(
            RichText::new("Blocked by constraints")
                .color(theme::danger())
                .font(theme::strong_font(10.5)),
        );
        for candidate in &suitability.blocked {
            ui.label(
                RichText::new(format!(
                    "{} — {}",
                    candidate.display_name,
                    candidate.warnings.join("; ")
                ))
                .color(theme::muted())
                .size(10.0),
            );
        }
    }
}

fn suitability_candidate_card(
    ui: &mut Ui,
    state: &mut RosteringState,
    action: &mut Option<RosteringAction>,
    shift_id: &str,
    candidate: &ApiSuitabilityCandidate,
) {
    Frame::new()
        .fill(theme::surface_raised())
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                contact_link(
                    ui,
                    &candidate.display_name,
                    &candidate.contact_id,
                    action,
                    true,
                );
                theme::pill(
                    ui,
                    &format!("{:.0}", candidate.score),
                    if candidate.meets_requirements {
                        theme::success()
                    } else {
                        theme::warning()
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Assign").color(theme::accent()).size(10.0),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        state.assignment_target = Some(ApiGraphNode {
                            kind: PartyKind::Contact,
                            id: candidate.contact_id.clone(),
                            label: candidate.display_name.clone(),
                            degree: 0,
                            depth: None,
                        });
                        state.assignment_role = "worker".to_owned();
                        state.assignment_pending = true;
                        *action = Some(RosteringAction::CreateAssignment {
                            shift_id: shift_id.to_owned(),
                            contact_id: candidate.contact_id.clone(),
                            role: "worker".to_owned(),
                        });
                    }
                });
            });
            for requirement in candidate
                .requirements
                .iter()
                .filter(|requirement| !requirement.is_met())
            {
                ui.label(
                    RichText::new(format!(
                        "! {} — {}",
                        requirement.quality_name,
                        requirement_status_label(requirement)
                    ))
                    .color(theme::warning())
                    .size(9.5),
                );
            }
            for reason in &candidate.reasons {
                ui.label(
                    RichText::new(format!("+ {reason}"))
                        .color(theme::muted())
                        .size(9.5),
                );
            }
            for warning in &candidate.warnings {
                ui.label(
                    RichText::new(format!("! {warning}"))
                        .color(theme::warning())
                        .size(9.5),
                );
            }
        });
}

fn contact_link(
    ui: &mut Ui,
    name: &str,
    contact_id: &str,
    action: &mut Option<RosteringAction>,
    strong: bool,
) {
    let text = if strong {
        RichText::new(name)
            .color(theme::accent())
            .font(theme::strong_font(11.5))
    } else {
        RichText::new(name).color(theme::accent())
    };
    if ui.add(egui::Button::new(text).frame(false)).clicked() {
        *action = Some(RosteringAction::OpenContact(contact_id.to_owned()));
    }
}

fn requirement_status_label(requirement: &ApiSuitabilityRequirement) -> String {
    let detail = requirement
        .detail
        .as_deref()
        .filter(|detail| !detail.is_empty())
        .map(|detail| format!(" ({detail})"))
        .unwrap_or_default();
    match requirement.status.as_str() {
        "expired" => format!("expired{detail}"),
        "mismatch" => format!("does not match{detail}"),
        "unknown" => format!("not recorded{detail}"),
        "expiring" => format!("expires soon{detail}"),
        _ => format!("required{detail}"),
    }
}

fn editor_card(
    ui: &mut Ui,
    editor: &mut ShiftEditor,
    sites: &[ApiSite],
    action: &mut Option<RosteringAction>,
) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if editor.editing_id.is_some() {
                    "Edit shift"
                } else {
                    "New shift"
                })
                .font(theme::strong_font(15.0)),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Close").color(theme::faint()).size(10.5))
                            .frame(false),
                    )
                    .clicked()
                {
                    editor.closed = true;
                }
            });
        });

        ui.horizontal_wrapped(|ui| {
            field_sized(ui, "Title", &mut editor.title, 180.0);
            ui.label(RichText::new("Kind").color(theme::muted()).size(10.5));
            let kind = egui::ComboBox::from_id_salt("shift-kind")
                .selected_text(shift_kind_label(&editor.kind))
                .width(150.0)
                .show_ui(ui, |ui| {
                    for kind in ["service_delivery", "opening_hours", "respite_program"] {
                        ui.selectable_value(
                            &mut editor.kind,
                            kind.to_owned(),
                            shift_kind_label(kind),
                        );
                    }
                });
            name_combo(&kind.response, "Shift kind");
            ui.label(RichText::new("Status").color(theme::muted()).size(10.5));
            let status = egui::ComboBox::from_id_salt("shift-status")
                .selected_text(status_label(&editor.status))
                .width(110.0)
                .show_ui(ui, |ui| {
                    for status in ["draft", "published", "cancelled", "completed"] {
                        ui.selectable_value(
                            &mut editor.status,
                            status.to_owned(),
                            status_label(status),
                        );
                    }
                });
            name_combo(&status.response, "Shift status");
        });

        ui.horizontal_wrapped(|ui| {
            field_sized(ui, "Date", &mut editor.date, 110.0);
            field_sized(ui, "Start", &mut editor.start, 70.0);
            field_sized(ui, "End", &mut editor.end, 70.0);

            ui.label(RichText::new("Site").color(theme::muted()).size(10.5));
            let site_label = editor
                .site_id
                .as_ref()
                .and_then(|id| sites.iter().find(|site| &site.id == id))
                .map(|site| site.name.clone())
                .unwrap_or_else(|| "No site".to_owned());
            let site = egui::ComboBox::from_id_salt("shift-site")
                .selected_text(site_label)
                .width(180.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(editor.site_id.is_none(), "No site")
                        .clicked()
                    {
                        editor.site_id = None;
                    }
                    for site in sites {
                        let selected = editor.site_id.as_deref() == Some(site.id.as_str());
                        if ui.selectable_label(selected, &site.name).clicked() {
                            editor.site_id = Some(site.id.clone());
                        }
                    }
                });
            name_combo(&site.response, "Shift site");
            field_sized(ui, "Location", &mut editor.location, 220.0);
        });
        field_sized(ui, "Notes", &mut editor.notes, 420.0);

        if editor.editing_id.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Repeats").color(theme::muted()).size(10.5));
                let recurrence = egui::ComboBox::from_id_salt("shift-recurrence")
                    .selected_text(match editor.recurrence.as_str() {
                        "daily" => "Daily",
                        "weekly" => "Weekly",
                        "fortnightly" => "Fortnightly",
                        _ => "Does not repeat",
                    })
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        for (value, label) in [
                            ("none", "Does not repeat"),
                            ("daily", "Daily"),
                            ("weekly", "Weekly"),
                            ("fortnightly", "Fortnightly"),
                        ] {
                            ui.selectable_value(&mut editor.recurrence, value.to_owned(), label);
                        }
                    });
                name_combo(&recurrence.response, "Recurrence");
                if editor.recurrence != "none" {
                    field_sized(ui, "Occurrences", &mut editor.recurrence_count, 60.0);
                    field_sized(ui, "or until", &mut editor.recurrence_until, 110.0);
                    ui.label(
                        RichText::new("Up to 100 shifts; dates use local wall time.")
                            .color(theme::faint())
                            .size(9.5),
                    );
                }
            });
        }

        if let Some(error) = &editor.error
            && !error.is_empty()
        {
            ui.label(RichText::new(error).color(theme::danger()).size(10.5));
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let label = if editor.saving {
                "Saving…"
            } else if editor.editing_id.is_some() {
                "Save shift"
            } else {
                "Create shift"
            };
            if ui
                .add_enabled(!editor.saving, theme::primary_button(label))
                .clicked()
            {
                editor.error = None;
                if let Some(id) = &editor.editing_id {
                    match editor.base_times() {
                        Ok((starts_at, ends_at)) => {
                            editor.saving = true;
                            *action = Some(RosteringAction::UpdateShift {
                                id: id.clone(),
                                payload: ApiShiftUpdate {
                                    title: trim_opt(&editor.title),
                                    kind: Some(editor.kind.clone()),
                                    starts_at: Some(starts_at),
                                    ends_at: Some(ends_at),
                                    location: trim_opt(&editor.location),
                                    status: Some(editor.status.clone()),
                                    division_id: None,
                                    service_id: None,
                                    site_id: editor.site_id.clone(),
                                    notes: trim_opt(&editor.notes),
                                },
                            });
                        }
                        Err(error) => editor.error = Some(error),
                    }
                } else {
                    match editor.build_batch() {
                        Ok((shifts, recurrence)) => {
                            editor.saving = true;
                            *action = Some(RosteringAction::CreateShifts { shifts, recurrence });
                        }
                        Err(error) => editor.error = Some(error),
                    }
                }
            }
            if ui.add(theme::secondary_button("Cancel")).clicked() {
                editor.closed = true;
            }
        });
    });
}

fn paste_card(ui: &mut Ui, paste: &mut PasteForm, action: &mut Option<RosteringAction>) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Paste shift").font(theme::strong_font(15.0)));
            ui.label(
                RichText::new(format!(
                    "from {} · {}",
                    local_date(&paste.source.starts_at),
                    paste
                        .source
                        .title
                        .clone()
                        .or_else(|| paste.source.service_name.clone())
                        .unwrap_or_else(|| shift_kind_label(&paste.source.kind))
                ))
                .color(theme::muted())
                .size(10.5),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Close").color(theme::faint()).size(10.5))
                            .frame(false),
                    )
                    .clicked()
                {
                    paste.closed = true;
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            field_sized(ui, "Date", &mut paste.date, 110.0);
            field_sized(ui, "Start", &mut paste.start, 70.0);
            field_sized(ui, "End", &mut paste.end, 70.0);
            ui.checkbox(
                &mut paste.include_assignments,
                RichText::new("Copy assignments").size(11.0),
            );
        });
        if let Some(error) = &paste.error
            && !error.is_empty()
        {
            ui.label(RichText::new(error).color(theme::danger()).size(10.5));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !paste.saving,
                    theme::primary_button(if paste.saving { "Pasting…" } else { "Paste" }),
                )
                .clicked()
            {
                match paste.build() {
                    Ok(payload) => {
                        paste.saving = true;
                        paste.error = None;
                        *action = Some(RosteringAction::CopyShift {
                            id: paste.source.id.clone(),
                            payload,
                        });
                    }
                    Err(error) => paste.error = Some(error),
                }
            }
            if ui.add(theme::secondary_button("Cancel")).clicked() {
                paste.closed = true;
            }
        });
    });
}

fn site_card(ui: &mut Ui, form: &mut SiteForm, action: &mut Option<RosteringAction>) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("New site").font(theme::strong_font(15.0)));
        ui.horizontal_wrapped(|ui| {
            field_sized(ui, "Name", &mut form.name, 220.0);
            field_sized(ui, "Address", &mut form.address, 280.0);
            if ui
                .add_enabled(!form.saving, theme::primary_button("Add site"))
                .clicked()
            {
                if form.name.trim().is_empty() {
                    form.error = Some("A site needs a name.".to_owned());
                } else {
                    form.saving = true;
                    form.error = None;
                    *action = Some(RosteringAction::CreateSite(ApiSiteWrite {
                        name: form.name.trim().to_owned(),
                        division_id: None,
                        address: trim_opt(&form.address),
                        notes: None,
                        is_active: Some(true),
                    }));
                }
            }
            if ui.add(theme::secondary_button("Cancel")).clicked() {
                form.closed = true;
            }
        });
        if let Some(error) = &form.error
            && !error.is_empty()
        {
            ui.label(RichText::new(error).color(theme::danger()).size(10.5));
        }
    });
}

fn meta_row(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme::faint()).size(10.0));
        ui.label(RichText::new(value).color(theme::text()).size(10.5));
    });
}

fn status_pill(ui: &mut Ui, status: &str) {
    theme::pill(
        ui,
        &status_label(status).to_uppercase(),
        status_colour(status),
    );
}

fn week_label(week_start: &str) -> String {
    let start = parse_local_date(week_start);
    let end = start + ChronoDuration::days(6);
    format!("{} – {}", start.format("%-d %b"), end.format("%-d %b %Y"))
}

fn field_sized(ui: &mut Ui, label: &str, value: &mut String, width: f32) {
    ui.label(RichText::new(label).color(theme::muted()).size(10.5));
    let response = ui.add_sized(
        [width, 30.0],
        egui::TextEdit::singleline(value).margin(Margin::symmetric(8, 6)),
    );
    name_input(&response, label);
}

fn name_input(response: &egui::Response, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
}

fn name_combo(response: &egui::Response, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
}

fn trim_opt(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn error_box(ui: &mut Ui, message: &str) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(
            theme::danger().r(),
            theme::danger().g(),
            theme::danger().b(),
            20,
        ))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.label(RichText::new(message).color(theme::danger()).size(11.0));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_rejects_reversed_times() {
        let mut editor = ShiftEditor::new_for("2026-06-15".to_owned());
        editor.start = "11:00".to_owned();
        editor.end = "09:00".to_owned();
        assert!(editor.base_times().is_err());

        editor.end = "12:00".to_owned();
        assert!(editor.base_times().is_ok());
    }

    #[test]
    fn weekly_recurrence_builds_occurrences() {
        let mut editor = ShiftEditor::new_for("2026-06-15".to_owned());
        editor.recurrence = "weekly".to_owned();
        editor.recurrence_count = "3".to_owned();
        let (shifts, recurrence) = editor.build_batch().expect("valid recurrence");
        assert_eq!(recurrence, "weekly");
        assert_eq!(shifts.len(), 3);
        assert_eq!(local_inputs(&shifts[1].starts_at).0, "2026-06-22");
        assert_eq!(local_inputs(&shifts[2].starts_at).0, "2026-06-29");
    }
}
