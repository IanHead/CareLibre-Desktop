//! Shared form controls with type-appropriate input widgets.

use eframe::egui::{self, Margin, RichText, Ui, vec2};

use crate::theme;

pub fn text_field(ui: &mut Ui, label: &str, value: &mut String) {
    if !label.is_empty() {
        field_label(ui, label, false);
    }
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(10, 8)),
    );
    if !label.is_empty() {
        named(&response, label);
    }
    ui.add_space(4.0);
}

pub fn email_field(ui: &mut Ui, label: &str, value: &mut String) {
    tagged_text_field(ui, label, value, "name@example.com");
}

pub fn phone_field(ui: &mut Ui, label: &str, value: &mut String) {
    tagged_text_field(ui, label, value, "Phone number");
}

pub fn url_field(ui: &mut Ui, label: &str, value: &mut String) {
    tagged_text_field(ui, label, value, "https://example.com");
}

fn tagged_text_field(ui: &mut Ui, label: &str, value: &mut String, hint: &str) {
    if !label.is_empty() {
        field_label(ui, label, false);
    }
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(10, 8)),
    );
    if !label.is_empty() {
        named(&response, label);
    }
    ui.add_space(4.0);
}

/// An integer or decimal field that rejects non-numeric characters as they are
/// typed. Integer fields also reject minus signs and decimal separators.
pub fn number_field(ui: &mut Ui, label: &str, value: &mut String, integer: bool) {
    if !label.is_empty() {
        field_label(ui, label, false);
    }
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(if integer { "0" } else { "0.00" })
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(10, 8)),
    );
    if response.changed() {
        *value = numeric_text(value, integer);
    }
    if !label.is_empty() {
        named(&response, label);
    }
    ui.add_space(4.0);
}

/// Digits only, for postcodes, ABNs and similar identifiers where leading
/// zeroes are meaningful.
pub fn digits_field(ui: &mut Ui, label: &str, value: &mut String) {
    if !label.is_empty() {
        field_label(ui, label, false);
    }
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text("Digits only")
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(10, 8)),
    );
    if response.changed() {
        value.retain(|character| character.is_ascii_digit());
    }
    if !label.is_empty() {
        named(&response, label);
    }
    ui.add_space(4.0);
}

/// A 24-hour time selector backed by hour and minute spinners.
pub fn time_field(ui: &mut Ui, label: &str, value: &mut String) {
    if !label.is_empty() {
        field_label(ui, label, false);
    }
    let (mut hour, mut minute) = parse_time(value).unwrap_or((9, 0));
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut hour)
                .range(0..=23)
                .speed(1.0)
                .custom_formatter(|value, _| format!("{value:02.0}")),
        );
        ui.label(":");
        ui.add(
            egui::DragValue::new(&mut minute)
                .range(0..=59)
                .speed(1.0)
                .custom_formatter(|value, _| format!("{value:02.0}")),
        );
        ui.label(RichText::new("24-hour").color(theme::faint()).size(10.0));
    });
    *value = format!("{hour:02}:{minute:02}");
    ui.add_space(4.0);
}

pub fn integer_field(ui: &mut Ui, label: &str, value: &mut String) {
    number_field(ui, label, value, true);
}

pub fn decimal_field(ui: &mut Ui, label: &str, value: &mut String) {
    number_field(ui, label, value, false);
}

fn numeric_text(value: &str, integer: bool) -> String {
    let mut result = String::new();
    let mut seen_decimal = false;
    for character in value.chars() {
        if character.is_ascii_digit() {
            result.push(character);
        } else if !integer && character == '.' && !seen_decimal {
            result.push('.');
            seen_decimal = true;
        }
    }
    result
}

fn parse_time(value: &str) -> Option<(i32, i32)> {
    let (hour, minute) = value.trim().split_once(':')?;
    let hour = hour.parse::<i32>().ok()?;
    let minute = minute.parse::<i32>().ok()?;
    (0..=23)
        .contains(&hour)
        .then_some(())
        .and_then(|_| (0..=59).contains(&minute).then_some((hour, minute)))
}

/// A validated date field with a calendar popup. The text stays editable, but
/// the value must parse as `YYYY-MM-DD` before it can be saved.
pub fn date_field(ui: &mut Ui, id: &str, label: &str, value: &mut String) {
    if !label.is_empty() {
        field_label(ui, label, false);
    }
    let field_id = ui.make_persistent_id(id);
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(value)
                .hint_text("YYYY-MM-DD")
                .desired_width(110.0)
                .margin(Margin::symmetric(8, 6)),
        );
        if !label.is_empty() {
            named(&response, label);
        }

        let picker = ui.add(
            egui::Button::new(RichText::new("📅").size(12.0))
                .frame(false)
                .min_size(vec2(24.0, 22.0)),
        );
        named(&picker, "Choose a date");

        let valid = value.trim().is_empty()
            || chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").is_ok();
        if !valid {
            ui.label(
                RichText::new("Use YYYY-MM-DD")
                    .color(theme::danger())
                    .size(10.0),
            );
        }

        egui::Popup::from_toggle_button_response(&picker).show(|ui| {
            if let Some(date) = calendar(ui, value, field_id) {
                *value = date.format("%Y-%m-%d").to_string();
                ui.close();
            }
            if ui.button("Clear").clicked() {
                value.clear();
                ui.close();
            }
        });
    });
    ui.add_space(4.0);
}

/// A compact month calendar. Returns the clicked date, if any.
fn calendar(ui: &mut Ui, value: &str, id: egui::Id) -> Option<chrono::NaiveDate> {
    use chrono::{Datelike, Months, NaiveDate};

    let today = chrono::Local::now().date_naive();
    let selected = NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok();
    let month_key = id.with("month");
    let mut month = ui
        .data(|data| data.get_temp::<NaiveDate>(month_key))
        .unwrap_or_else(|| selected.unwrap_or(today))
        .with_day(1)
        .unwrap_or(today);

    let mut picked = None;
    ui.horizontal(|ui| {
        if ui.small_button("◀").clicked() {
            month = month.checked_sub_months(Months::new(1)).unwrap_or(month);
        }
        ui.label(RichText::new(month.format("%B %Y").to_string()).font(theme::strong_font(11.0)));
        if ui.small_button("▶").clicked() {
            month = month.checked_add_months(Months::new(1)).unwrap_or(month);
        }
    });
    ui.data_mut(|data| data.insert_temp(month_key, month));

    egui::Grid::new(id.with("grid"))
        .num_columns(7)
        .spacing([2.0, 2.0])
        .show(ui, |ui| {
            for day in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                ui.label(RichText::new(day).color(theme::faint()).size(9.0));
            }
            ui.end_row();

            let offset = month.weekday().num_days_from_monday() as i64;
            let next_month = month.checked_add_months(Months::new(1)).unwrap_or(month);
            let days = (next_month - month).num_days();

            let mut column = 0;
            for _ in 0..offset {
                ui.label("");
                column += 1;
            }
            for day in 0..days {
                let date = month + chrono::Duration::days(day);
                let is_selected = selected == Some(date);
                let is_today = date == today;
                let text = if is_today {
                    RichText::new(date.day().to_string())
                        .color(theme::accent())
                        .strong()
                } else {
                    RichText::new(date.day().to_string())
                };
                if ui.selectable_label(is_selected, text).clicked() {
                    picked = Some(date);
                }
                column += 1;
                if column % 7 == 0 {
                    ui.end_row();
                }
            }
        });

    picked
}

pub fn field_label(ui: &mut Ui, label: &str, required: bool) {
    if label.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(theme::muted()).size(10.5));
        if required {
            ui.label(RichText::new("*").color(theme::accent()).size(10.5));
        }
    });
    ui.add_space(3.0);
}

pub fn named(response: &egui::Response, label: &str) {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, label));
}

// Label-free controls for forms that draw their own labels.

pub fn text_input(ui: &mut Ui, value: &mut String, hint: &str) -> bool {
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(10, 8)),
    )
    .changed()
}

pub fn email_input(ui: &mut Ui, value: &mut String) -> bool {
    text_input(ui, value, "name@example.com")
}

pub fn phone_input(ui: &mut Ui, value: &mut String) -> bool {
    text_input(ui, value, "Phone number")
}

pub fn url_input(ui: &mut Ui, value: &mut String) -> bool {
    text_input(ui, value, "https://example.com")
}

pub fn digits_input(ui: &mut Ui, value: &mut String) -> bool {
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text("Digits only")
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(10, 8)),
    );
    if response.changed() {
        value.retain(|character| character.is_ascii_digit());
    }
    response.changed()
}

pub fn number_input(ui: &mut Ui, value: &mut String, integer: bool) -> bool {
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(if integer { "0" } else { "0.00" })
            .desired_width(ui.available_width())
            .margin(Margin::symmetric(10, 8)),
    );
    if response.changed() {
        *value = numeric_text(value, integer);
    }
    response.changed()
}

pub fn time_input(ui: &mut Ui, value: &mut String) -> bool {
    let before = value.clone();
    let (mut hour, mut minute) = parse_time(value).unwrap_or((9, 0));
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut hour)
                .range(0..=23)
                .speed(1.0)
                .custom_formatter(|value, _| format!("{value:02.0}")),
        );
        ui.label(":");
        ui.add(
            egui::DragValue::new(&mut minute)
                .range(0..=59)
                .speed(1.0)
                .custom_formatter(|value, _| format!("{value:02.0}")),
        );
    });
    *value = format!("{hour:02}:{minute:02}");
    *value != before
}

pub fn date_input(ui: &mut Ui, id: &str, value: &mut String) -> bool {
    let field_id = ui.make_persistent_id(id);
    let before = value.clone();
    ui.horizontal(|ui| {
        let response = ui.add(
            egui::TextEdit::singleline(value)
                .hint_text("YYYY-MM-DD")
                .desired_width(110.0)
                .margin(Margin::symmetric(8, 6)),
        );
        named(&response, "Date");
        let picker = ui.add(
            egui::Button::new(RichText::new("📅").size(12.0))
                .frame(false)
                .min_size(vec2(24.0, 22.0)),
        );
        named(&picker, "Choose a date");
        egui::Popup::from_toggle_button_response(&picker).show(|ui| {
            if let Some(date) = calendar(ui, value, field_id) {
                *value = date.format("%Y-%m-%d").to_string();
                ui.close();
            }
            if ui.button("Clear").clicked() {
                value.clear();
                ui.close();
            }
        });
    });
    *value != before
}

pub fn number_sized(ui: &mut Ui, value: &mut String, width: f32, integer: bool) -> bool {
    let response = ui.add_sized(
        [width, 30.0],
        egui::TextEdit::singleline(value).hint_text(if integer { "0" } else { "0.00" }),
    );
    if response.changed() {
        *value = numeric_text(value, integer);
    }
    response.changed()
}
