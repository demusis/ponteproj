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
        "original" => "Documento original",
        "attachment" => "Anexo",
        "antecedent" => "Documento anterior",
        _ => "Referência",
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

/// Tipo de item como aparece nas filas e históricos da rede.
pub fn item_kind(kind: &str) -> &str {
    match kind {
        "document" => "documento",
        "capture" => "importação",
        "task" => "tarefa",
        "note" => "notas",
        "draft" => "minuta",
        "analysis" => "análise",
        "work_context" => "contexto",
        "saiop" => "atendimento",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_dates_are_normalized_to_iso() {
        assert_eq!(parse_typed_date("03/11/2026").as_deref(), Some("2026-11-03"));
        assert_eq!(parse_typed_date("3/11/2026").as_deref(), Some("2026-11-03"));
        assert_eq!(parse_typed_date("03-11-2026").as_deref(), Some("2026-11-03"));
        assert_eq!(parse_typed_date("2026-11-03").as_deref(), Some("2026-11-03"));
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
        assert_eq!(saiop_event_kind("importacao_planilha"), "Importação de planilha");
        assert_eq!(spreadsheet_column("D"), "Referência");
        assert_eq!(elapsed("1990-01-01T00:00:00Z").unwrap().contains("dias"), true);
    }
}
