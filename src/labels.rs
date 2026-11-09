//! Rótulos e formatos exibidos ao usuário. Os valores internos (chaves em
//! minúsculas, datas ISO/RFC3339) nunca chegam à interface sem passar por aqui.

/// `AAAA-MM-DD` → `dd/mm/aaaa`. Texto que não é data é devolvido como está.
pub fn date(value: &str) -> String {
    let value = value.trim();
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(|d| d.format("%d/%m/%Y").to_string())
        .unwrap_or_else(|_| value.to_owned())
}

/// RFC3339 → `dd/mm/aaaa HH:MM` no fuso local. Também aceita `AAAA-MM-DD`.
pub fn datetime(value: &str) -> String {
    let value = value.trim();
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(value) {
        return parsed
            .with_timezone(&chrono::Local)
            .format("%d/%m/%Y %H:%M")
            .to_string();
    }
    date(value)
}

/// Tempo decorrido em linguagem corrente ("há 3 min", "há 2 h", "ontem").
pub fn elapsed(rfc3339: &str) -> Option<String> {
    let then = chrono::DateTime::parse_from_rfc3339(rfc3339.trim()).ok()?;
    let seconds = (chrono::Utc::now() - then.with_timezone(&chrono::Utc)).num_seconds();
    Some(match seconds {
        s if s < 0 => "agora".into(),
        s if s < 60 => "agora".into(),
        s if s < 3600 => format!("há {} min", s / 60),
        s if s < 86_400 => format!("há {} h", s / 3600),
        s if s < 172_800 => "ontem".into(),
        s => format!("há {} dias", s / 86_400),
    })
}

pub fn draft_kind(kind: &str) -> &str {
    match kind {
        "resposta" => "Resposta",
        "encaminhamento" => "Encaminhamento",
        "complementacao" => "Pedido de complementação",
        other => other,
    }
}

pub fn priority(value: &str) -> &str {
    match value {
        "" => "Não definida",
        "baixa" => "Baixa",
        "normal" => "Normal",
        "alta" => "Alta",
        "urgente" => "Urgente",
        "não classificada" => "Não classificada",
        other => other,
    }
}

pub fn task_status(value: &str) -> &str {
    crate::tasks::status_label(value)
}

pub fn task_action(value: &str) -> &str {
    crate::tasks::action_label(value)
}

pub fn purpose(value: &str) -> &str {
    crate::work_context::purpose_label(value)
}

pub fn reference_kind(kind: &str) -> &str {
    match kind {
        "original" => "Original no SIGADOC",
        "attachment" => "Anexo no SIGADOC",
        "antecedent" => "Documento anterior",
        _ => "Link",
    }
}

/// Origin tag of a PDF copy stored locally (as opposed to the link itself).
pub fn downloaded_kind(kind: &str) -> &str {
    match kind {
        "original" => "cópia do documento original",
        "attachment" => "cópia de anexo",
        _ => "cópia baixada",
    }
}

pub fn link_kind(kind: &str) -> &str {
    match kind {
        "antecedente" => "Documento anterior",
        "resposta" => "Resposta a",
        "relacionado" => "Relacionado",
        other => other,
    }
}

pub fn saiop_event_kind(kind: &str) -> &str {
    match kind {
        "entrada" => "Entrada",
        "situacao" => "Situação",
        "distribuicao" => "Responsável",
        "anotacao" => "Anotação",
        "importacao_planilha" => "Importação de planilha",
        "tarefa" => "Tarefa",
        "mesclagem" => "Mesclagem de versões",
        other => other,
    }
}

/// Nome legível do campo de uma linha importada da planilha (coluna → rótulo).
pub fn spreadsheet_column(column: &str) -> &str {
    match column {
        "B" => "Entrada",
        "C" => "Responsável",
        "D" => "Referência",
        "E" => "Fórmula auxiliar (E)",
        "F" => "Código SIGADOC (F)",
        "G" => "Código SIGADOC (G)",
        "H" => "Fórmula auxiliar (H)",
        "I" => "Recebido de",
        "J" => "Retorno para UAS?",
        "K" => "Prazo",
        "L" => "Observações",
        "M" => "Verificação",
        "N" => "Assinado?",
        "O" => "Data de saída",
        "P" => "Entrega prevista",
        "Q" => "Enviado para",
        "R" => "Assunto",
        "S" => "Coluna S",
        other => other,
    }
}

/// Tamanho de arquivo em linguagem corrente: "830 KB", "12,4 MB".
pub fn file_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let value = bytes as f64;
    if value >= MB {
        // One decimal unless it is a whole number ("8 MB", "12,4 MB").
        let mb = (value / MB * 10.0).round() / 10.0;
        if mb.fract() == 0.0 {
            format!("{} MB", mb as u64)
        } else {
            format!("{mb:.1} MB").replace('.', ",")
        }
    } else if value >= KB {
        format!("{} KB", (value / KB).round() as u64)
    } else {
        format!("{bytes} bytes")
    }
}

/// Tipo de item como aparece nas filas e históricos da rede.
pub fn item_kind(kind: &str) -> &str {
    match kind {
        "attachment" => "anexo",
        "document" => "documento",
        "capture" => "importação",
        "task" => "tarefa",
        "note" => "notas",
        "draft" => "minuta",
        "analysis" => "análise",
        "work_context" => "contexto",
        "saiop" => "atendimento",
        "example" => "exemplo",
        "saiop_batch" => "lote de planilha",
        "document_link" => "vínculo",
        other => other,
    }
}

/// Parses what a person types as a date: `dd/mm/aaaa`, `dd/mm/aa`, `dd-mm-aaaa`
/// or `aaaa-mm-dd`. Returns the ISO form used in storage.
pub fn parse_typed_date(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    use chrono::Datelike;
    for format in ["%d/%m/%Y", "%d-%m-%Y", "%d.%m.%Y", "%Y-%m-%d"] {
        // `%Y` also accepts "26"; a two-digit year is handled by `%y` below.
        if let Ok(date) = chrono::NaiveDate::parse_from_str(text, format) {
            if date.year() >= 1900 {
                return Some(date.format("%Y-%m-%d").to_string());
            }
        }
    }
    for format in ["%d/%m/%y", "%d-%m-%y", "%d.%m.%y"] {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(text, format) {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }
    None
}

#[cfg(feature = "desktop")]
/// Text field for a date. The person sees and types `dd/mm/aaaa`; the backing
/// string keeps `AAAA-MM-DD` (or the raw text when invalid, so that saving is
/// refused with a clear message instead of silently keeping an old value).
pub fn date_field(
    ui: &mut eframe::egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
) -> eframe::egui::Response {
    use eframe::egui;
    let id = ui.make_persistent_id(("ponte-date-field", id_salt));
    // (buffer shown, backing value it was derived from)
    let mut state: (String, String) = ui
        .data(|d| d.get_temp::<(String, String)>(id))
        .unwrap_or_default();
    if state.1 != *value {
        state = (date(value), value.clone());
    }
    let valid = value.trim().is_empty() || parse_typed_date(value).is_some();
    let response = ui
        .horizontal(|ui| {
            let mut edit = egui::TextEdit::singleline(&mut state.0)
                .hint_text("dd/mm/aaaa")
                .desired_width(96.0);
            if !valid {
                edit = edit.text_color(egui::Color32::from_rgb(168, 40, 45));
            }
            let response = ui.add(edit);
            if response.changed() {
                *value = match parse_typed_date(&state.0) {
                    Some(iso) => iso,
                    None if state.0.trim().is_empty() => String::new(),
                    None => state.0.clone(),
                };
                state.1 = value.clone();
            }
            let picker = ui
                .small_button("📅")
                .on_hover_text("Escolher a data num calendário");
            if let Some(chosen) = calendar_popup(id, &picker, value) {
                *value = chosen;
                state = (date(value), value.clone());
            }
            if ui
                .small_button("Hoje")
                .on_hover_text("Preenche com a data de hoje")
                .clicked()
            {
                *value = chrono::Local::now().format("%Y-%m-%d").to_string();
                state = (date(value), value.clone());
            }
            if !valid {
                ui.colored_label(egui::Color32::from_rgb(168, 40, 45), "⚠ data inválida")
                    .on_hover_text("Use o formato dd/mm/aaaa.");
            }
            response
        })
        .inner;
    ui.data_mut(|d| d.insert_temp(id, state));
    response
}

#[cfg(feature = "desktop")]
const MONTHS: [&str; 12] = [
    "Janeiro",
    "Fevereiro",
    "Março",
    "Abril",
    "Maio",
    "Junho",
    "Julho",
    "Agosto",
    "Setembro",
    "Outubro",
    "Novembro",
    "Dezembro",
];
#[cfg(feature = "desktop")]
const WEEKDAYS: [&str; 7] = ["D", "S", "T", "Q", "Q", "S", "S"];

/// The weeks of a month as rows of seven cells, Sunday first, `None` where
/// the cell belongs to another month.
pub fn month_grid(year: i32, month: u32) -> Vec<[Option<u32>; 7]> {
    use chrono::Datelike;
    let Some(first) = chrono::NaiveDate::from_ymd_opt(year, month, 1) else {
        return Vec::new();
    };
    let days = (1..=31)
        .filter(|d| chrono::NaiveDate::from_ymd_opt(year, month, *d).is_some())
        .count() as u32;
    let offset = first.weekday().num_days_from_sunday() as usize;
    let mut rows = Vec::new();
    let mut row = [None; 7];
    let mut column = offset;
    for day in 1..=days {
        row[column] = Some(day);
        column += 1;
        if column == 7 {
            rows.push(row);
            row = [None; 7];
            column = 0;
        }
    }
    if column > 0 {
        rows.push(row);
    }
    rows
}

#[cfg(feature = "desktop")]
/// Calendar shown under the 📅 button: month and year to browse, weekday
/// headings, one button per day; today and the chosen date stand out.
/// Returns the ISO date when a day is clicked.
fn calendar_popup(
    field_id: eframe::egui::Id,
    button: &eframe::egui::Response,
    value: &str,
) -> Option<String> {
    use chrono::Datelike;
    use eframe::egui;
    let today = chrono::Local::now().date_naive();
    let selected = parse_typed_date(value)
        .and_then(|iso| chrono::NaiveDate::parse_from_str(&iso, "%Y-%m-%d").ok());
    let view_id = field_id.with("calendar-view");
    let mut chosen = None;
    egui::Popup::from_toggle_button_response(button)
        .id(field_id.with("calendar"))
        .show(|ui| {
            let base = selected.unwrap_or(today);
            let (mut year, mut month) = ui
                .data(|d| d.get_temp::<(i32, u32)>(view_id))
                .unwrap_or((base.year(), base.month()));
            // Exactly seven columns wide: the popup must not stretch to the
            // panel's width and leave a blank band beside the days.
            const CELL: f32 = 28.0;
            const WIDTH: f32 = CELL * 7.0 + 2.0 * 6.0;
            ui.set_width(WIDTH);
            ui.horizontal(|ui| {
                let previous = ui.small_button("◀").on_hover_text("Mês anterior");
                if previous.clicked() {
                    if month == 1 {
                        month = 12;
                        year -= 1;
                    } else {
                        month -= 1;
                    }
                }
                let arrows = previous.rect.width() + ui.spacing().item_spacing.x;
                ui.add_sized(
                    [WIDTH - 2.0 * arrows, previous.rect.height()],
                    egui::Label::new(
                        egui::RichText::new(format!("{} {year}", MONTHS[(month - 1) as usize]))
                            .strong(),
                    )
                    .selectable(false),
                );
                if ui.small_button("▶").on_hover_text("Mês seguinte").clicked() {
                    if month == 12 {
                        month = 1;
                        year += 1;
                    } else {
                        month += 1;
                    }
                }
            });
            egui::Grid::new(view_id.with("grid"))
                .num_columns(7)
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    for name in WEEKDAYS {
                        ui.add_sized(
                            [CELL, 18.0],
                            egui::Label::new(
                                egui::RichText::new(name)
                                    .small()
                                    .color(egui::Color32::from_rgb(120, 130, 140)),
                            ),
                        );
                    }
                    ui.end_row();
                    for week in month_grid(year, month) {
                        for cell in week {
                            match cell {
                                Some(day) => {
                                    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)
                                        .expect("valid day");
                                    let is_selected = selected == Some(date);
                                    let is_today = date == today;
                                    let mut text = egui::RichText::new(day.to_string());
                                    if is_today {
                                        text = text
                                            .strong()
                                            .color(egui::Color32::from_rgb(24, 84, 97));
                                    }
                                    let response = ui.add_sized(
                                        [CELL, 22.0],
                                        egui::Button::selectable(is_selected, text),
                                    );
                                    if response.clicked() {
                                        chosen = Some(date.format("%Y-%m-%d").to_string());
                                        ui.close();
                                    }
                                }
                                None => {
                                    ui.add_sized([CELL, 22.0], egui::Label::new(""));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            ui.horizontal(|ui| {
                if ui.small_button("Hoje").clicked() {
                    chosen = Some(today.format("%Y-%m-%d").to_string());
                    ui.close();
                }
                if !value.trim().is_empty() && ui.small_button("Limpar").clicked() {
                    chosen = Some(String::new());
                    ui.close();
                }
                ui.small(format!("Hoje: {}", today.format("%d/%m/%Y")));
            });
            ui.data_mut(|d| d.insert_temp(view_id, (year, month)));
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_grid_starts_on_sunday_and_knows_leap_years() {
        // 1 November 2026 is a Sunday; the month has 30 days.
        let november = month_grid(2026, 11);
        assert_eq!(november[0][0], Some(1));
        assert_eq!(november.last().unwrap().iter().flatten().last(), Some(&30));
        assert_eq!(november.len(), 5);
        // February 2024 (leap year): 29 days, starting on a Thursday.
        let february = month_grid(2024, 2);
        assert_eq!(
            february[0],
            [None, None, None, None, Some(1), Some(2), Some(3)]
        );
        assert_eq!(february.iter().flatten().flatten().count(), 29);
        assert!(month_grid(2026, 13).is_empty());
    }

    #[test]
    fn typed_dates_are_normalized_to_iso() {
        assert_eq!(
            parse_typed_date("03/11/2026").as_deref(),
            Some("2026-11-03")
        );
        assert_eq!(parse_typed_date("3/11/2026").as_deref(), Some("2026-11-03"));
        assert_eq!(
            parse_typed_date("03-11-2026").as_deref(),
            Some("2026-11-03")
        );
        assert_eq!(
            parse_typed_date("2026-11-03").as_deref(),
            Some("2026-11-03")
        );
        assert_eq!(parse_typed_date("03/11/26").as_deref(), Some("2026-11-03"));
        assert_eq!(parse_typed_date("31/02/2026"), None);
        assert_eq!(parse_typed_date("amanhã"), None);
        assert_eq!(parse_typed_date("  "), None);
    }

    #[test]
    fn dates_and_keys_are_rendered_in_user_terms() {
        assert_eq!(date("2026-11-03"), "03/11/2026");
        assert_eq!(date("texto livre"), "texto livre");
        assert_eq!(date(""), "");
        assert_eq!(datetime("2026-11-03"), "03/11/2026");
        assert!(datetime("2026-11-03T14:22:01.123+00:00").starts_with("03/11/2026 "));
        assert_eq!(priority(""), "Não definida");
        assert_eq!(priority("alta"), "Alta");
        assert_eq!(draft_kind("complementacao"), "Pedido de complementação");
        assert_eq!(
            saiop_event_kind("importacao_planilha"),
            "Importação de planilha"
        );
        assert_eq!(spreadsheet_column("D"), "Referência");
        assert_eq!(file_size(512), "512 bytes");
        assert_eq!(file_size(850_000), "830 KB");
        assert_eq!(file_size(8 * 1024 * 1024), "8 MB");
        assert_eq!(file_size(13_000_000), "12,4 MB");
        assert_eq!(file_size(25 * 1024 * 1024), "25 MB");
        assert_eq!(file_size(9 * 1024 * 1024 + 512 * 1024), "9,5 MB");
        assert!(elapsed("1990-01-01T00:00:00Z").unwrap().contains("dias"));
    }
}
