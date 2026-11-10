//! The drafts tab (minutas) and the windows that review what the AI proposes.
use super::{
    display_date, facts, info, kind_label, notice_line, pill, titled_card, Leave, PonteApp,
    Replacement, WorkKind,
};
use crate::ai;
use crate::model::DocumentDetail;
use crate::theme::{self, icon, S2, S3};
use crate::widgets::{self, Tone};
use eframe::egui::{self, RichText};

impl PonteApp {
    pub(super) fn drafts(&mut self, ui: &mut egui::Ui, detail: &DocumentDetail) {
        let editor = &self.editors[&detail.summary.id];
        let export_capture = if editor.capture_id == detail.summary.latest_capture_id {
            detail.capture.clone()
        } else {
            match self
                .store
                .load_capture(detail.summary.id, editor.capture_id)
            {
                Ok(c) => c,
                Err(e) => {
                    widgets::muted(
                        ui,
                        format!("Não foi possível ler a importação da minuta: {e}"),
                    );
                    return;
                }
            }
        };
        let id = detail.summary.id;
        let mut template = false;
        let mut generate: Option<crate::work_templates::Intent> = None;
        let mut save_as_template = false;
        let mut material: Option<String> = None;
        let mut save = false;
        let mut loaded = None;
        // Published work templates of the editor's kind, visible to this member.
        let member = self
            .store
            .load_intranet_config()
            .map(|c| c.member_name)
            .unwrap_or_default();
        let current_kind = self.editors[&id].kind.clone();
        let offered: Vec<crate::work_templates::WorkTemplate> = self
            .store
            .list_templates()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.offered_for(&current_kind, &member))
            .collect();
        let blocks = if offered.is_empty() {
            Vec::new()
        } else {
            self.store.list_blocks().unwrap_or_default()
        };
        // A lexical hint: the offered template whose words match the
        // document's, shown only while no template is chosen.
        let suggested = if self.editors[&id].template_uid.is_none() && !offered.is_empty() {
            self.store
                .suggest_template(&detail.capture, &current_kind, &member)
                .ok()
                .flatten()
        } else {
            None
        };
        let mut export = false;
        let mut compare: Option<crate::model::DraftVersion> = None;
        let busy_generating = self
            .worker
            .as_ref()
            .is_some_and(|w| w.kind == WorkKind::Draft);
        let generating_label = self
            .worker
            .as_ref()
            .filter(|w| w.kind == WorkKind::Draft)
            .map(|w| format!("Gerando… {}", w.short_step()));
        let disclosure = self.ai_disclosure_text(true);
        let suggestion_pending = self.suggestion.is_some();
        titled_card(
            ui,
            "Minuta",
            |ui| {
                info(ui, "Texto editável para revisão. Confira fatos, destinatário e fundamentos antes de usar no SIGADOC. Nada é enviado ao SIGADOC por aqui.");
            },
            |ui| {
                {
                    let editor = self.editors.get_mut(&id).expect("editor exists");
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = S3;
                        widgets::field_label(ui, "Tipo");
                        egui::ComboBox::from_id_salt("draft-kind")
                            .selected_text(kind_label(&editor.kind).to_owned())
                            .show_ui(ui, |ui| {
                                for kind in ["resposta", "encaminhamento", "complementacao"] {
                                    ui.selectable_value(&mut editor.kind, kind.into(), kind_label(kind));
                                }
                            });
                        widgets::field_label(ui, "Modelo de trabalho");
                        // A template no longer offered (other kind, archived or
                        // unpublished meanwhile) is dropped, not kept hidden.
                        if editor
                            .template_uid
                            .as_ref()
                            .is_some_and(|uid| !offered.iter().any(|t| &t.uid == uid))
                        {
                            editor.template_uid = None;
                            editor.selected_blocks.clear();
                        }
                        let chosen = editor
                            .template_uid
                            .as_ref()
                            .and_then(|uid| offered.iter().find(|t| &t.uid == uid));
                        let chosen_name = chosen
                            .map(|t| t.name.clone())
                            .unwrap_or_else(|| "Nenhum".into());
                        egui::ComboBox::from_id_salt("draft-template")
                            .selected_text(chosen_name)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(editor.template_uid.is_none(), "Nenhum")
                                    .clicked()
                                {
                                    editor.template_uid = None;
                                    editor.selected_blocks.clear();
                                }
                                for candidate in &offered {
                                    let selected = editor.template_uid.as_deref() == Some(&candidate.uid);
                                    let response = ui.selectable_label(selected, &candidate.name);
                                    let response = if candidate.purpose.is_empty() {
                                        response
                                    } else {
                                        response.on_hover_text(&candidate.purpose)
                                    };
                                    if response.clicked() {
                                        editor.template_uid = Some(candidate.uid.clone());
                                        editor.selected_blocks.clear();
                                    }
                                }
                            });
                        template = ui
                            .add(widgets::secondary("Começar por um modelo"))
                            .on_hover_text("Preenche o editor com a estrutura do modelo escolhido (ou um texto-base) sem usar IA.")
                            .clicked();
                        save_as_template = ui
                            .add_enabled(
                                !editor.body.trim().is_empty(),
                                widgets::quiet("Guardar como modelo"),
                            )
                            .on_hover_text("Leva este texto para a área Modelos como rascunho de um novo modelo de trabalho.")
                            .clicked();
                    });
                    if let Some(ranked) = &suggested {
                        ui.horizontal_wrapped(|ui| {
                            widgets::small_muted(ui, format!(
                                "Modelo sugerido pelas palavras do documento: {} ({}% de semelhança)",
                                ranked.item.name,
                                ranked.percent()
                            ))
                            .on_hover_text("Comparação lexical entre o documento e o nome, a finalidade, os critérios de uso e os exemplos aprovados de cada modelo publicado. É só uma sugestão.");
                            if ui.add(widgets::quiet("Usar este modelo")).clicked() {
                                editor.template_uid = Some(ranked.item.uid.clone());
                                editor.selected_blocks.clear();
                            }
                        });
                    }
                    // Optional and conditional blocks of the chosen template.
                    if let Some(version) = editor
                        .template_uid
                        .as_ref()
                        .and_then(|uid| offered.iter().find(|t| &t.uid == uid))
                        .and_then(|t| t.published_version())
                    {
                        use crate::work_templates::BlockMode;
                        let optional: Vec<_> = version
                            .block_refs
                            .iter()
                            .filter(|r| r.mode != BlockMode::Mandatory)
                            .collect();
                        if !optional.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                widgets::field_label(ui, "Instruções opcionais");
                                for reference in optional {
                                    let name = blocks
                                        .iter()
                                        .find(|b| b.uid == reference.block_uid)
                                        .map(|b| b.name.clone())
                                        .unwrap_or_else(|| "instrução".into());
                                    let label = match &reference.mode {
                                        BlockMode::Conditional { condition } => {
                                            format!("{name} (se: {condition})")
                                        }
                                        _ => name,
                                    };
                                    let mut on =
                                        editor.selected_blocks.contains(&reference.block_uid);
                                    if ui.checkbox(&mut on, label).changed() {
                                        if on {
                                            editor
                                                .selected_blocks
                                                .insert(reference.block_uid.clone());
                                        } else {
                                            editor.selected_blocks.remove(&reference.block_uid);
                                        }
                                    }
                                }
                            });
                        }
                    }
                    widgets::field_label(ui, "Pedido desta minuta (opcional)");
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.instructions)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY)
                            .hint_text("Ex.: solicitar a informação que falta, com tom objetivo."),
                    );
                    ui.horizontal_wrapped(|ui| {
                        use crate::work_templates::Intent;
                        let can_generate = !busy_generating && !suggestion_pending;
                        let has_text = !editor.body.trim().is_empty();
                        let disabled_reason = if suggestion_pending {
                            "Há uma sugestão pronta aguardando revisão (barra inferior)."
                        } else {
                            "Aguarde a geração em andamento."
                        };
                        if ui
                            .add_enabled(
                                can_generate,
                                widgets::primary(
                                    generating_label
                                        .clone()
                                        .unwrap_or_else(|| format!("{} Gerar com IA", icon::AI)),
                                ),
                            )
                            .on_hover_text(&disclosure)
                            .on_disabled_hover_text(disabled_reason)
                            .clicked()
                        {
                            generate = Some(Intent::New);
                        }
                        if ui
                            .add_enabled(
                                can_generate && has_text,
                                widgets::secondary("Completar"),
                            )
                            .on_hover_text("Completar texto atual: a IA preserva o que você escreveu, preenche os [preencher] e acrescenta as seções que faltam.")
                            .on_disabled_hover_text(if has_text { disabled_reason } else { "Escreva ou carregue um texto antes." })
                            .clicked()
                        {
                            generate = Some(Intent::Complete);
                        }
                        let has_request = !editor.instructions.trim().is_empty();
                        if ui
                            .add_enabled(
                                can_generate && has_text && has_request,
                                widgets::secondary("Revisar"),
                            )
                            .on_hover_text("Revisar texto atual: a IA altera só o necessário para atender ao pedido; o resultado abre lado a lado com o texto atual.")
                            .on_disabled_hover_text(if !has_text {
                                "Escreva ou carregue um texto antes."
                            } else if !has_request {
                                "Escreva em \"Pedido desta minuta\" o que deve ser revisado."
                            } else {
                                disabled_reason
                            })
                            .clicked()
                        {
                            generate = Some(Intent::Revise);
                        }
                    });
                }
                {
                    let editor = self.editors.get_mut(&id).expect("editor exists");
                    if editor.capture_id != detail.summary.latest_capture_id
                        || self.store.sources_stale(&editor.materials)
                    {
                        notice_line(ui, Tone::Warn, "Este texto foi feito sobre uma importação anterior ou fontes que mudaram. Revise antes de aproveitar.");
                    }
                    ui.add(
                        egui::TextEdit::multiline(&mut editor.body)
                            .desired_rows(16)
                            .desired_width(f32::INFINITY)
                            .hint_text("Escreva livremente ou comece por um modelo…"),
                    );
                    ui.horizontal_wrapped(|ui| {
                        save = ui
                            .add_enabled(
                                editor.draft_dirty() && !editor.body.trim().is_empty(),
                                widgets::primary(format!("{} Salvar nova versão", icon::SAVE)),
                            )
                            .clicked();
                        if ui
                            .add_enabled(
                                !editor.body.is_empty(),
                                widgets::secondary(format!("{} Copiar", icon::COPY)),
                            )
                            .on_hover_text("Copiar texto. Não salva uma versão.")
                            .clicked()
                        {
                            ui.ctx().copy_text(editor.body.clone());
                        }
                        export = ui
                            .add_enabled(
                                !editor.body.trim().is_empty(),
                                widgets::secondary("Exportar (RTF)"),
                            )
                            .on_hover_text("Cria uma cópia legível fora do acervo protegido, para abrir no Word ou similar. Não salva uma versão.")
                            .clicked();
                        if !detail.drafts.is_empty() {
                            egui::containers::menu::MenuButton::from_button(widgets::secondary(
                                format!("Comparar com versão {}", icon::DOWN),
                            ))
                            .ui(ui, |ui| {
                                for draft in &detail.drafts {
                                    if ui
                                        .button(format!(
                                            "#{} • {} • {}",
                                            draft.id,
                                            kind_label(&draft.kind),
                                            display_date(&draft.created_at)
                                        ))
                                        .clicked()
                                    {
                                        compare = Some(draft.clone());
                                        ui.close();
                                    }
                                }
                            });
                        }
                        if editor.draft_dirty() {
                            pill(ui, "não salva", Tone::Warn);
                        }
                        widgets::small_muted(ui, editor.origin.as_str());
                    });
                }
                ui.collapsing(format!("Versões salvas ({})", detail.drafts.len()), |ui| {
                    for (index, draft) in detail.drafts.iter().enumerate() {
                        if index > 0 {
                            ui.separator();
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.add(widgets::secondary("Abrir")).clicked() {
                                loaded = Some(draft.clone());
                            }
                            ui.label(format!(
                                "#{} • {} • {}",
                                draft.id,
                                kind_label(&draft.kind),
                                display_date(&draft.created_at)
                            ));
                            if draft.stale || draft.capture_id != detail.summary.latest_capture_id {
                                pill(ui, "importação anterior", Tone::Warn);
                            }
                            if let Some(provenance) = &draft.provenance {
                                if let Some(uid) = &provenance.generation_uid {
                                    if ui
                                        .add(widgets::quiet("Material utilizado"))
                                        .on_hover_text("Modelo, instruções, pedido, exemplos, fontes e IA que produziram este texto.")
                                        .clicked()
                                    {
                                        material = Some(uid.clone());
                                    }
                                } else if !provenance.template_name.is_empty() {
                                    widgets::small_muted(
                                        ui,
                                        format!("modelo {}", provenance.template_name),
                                    );
                                }
                            }
                        });
                    }
                    if detail.drafts.is_empty() {
                        widgets::small_muted(
                            ui,
                            "As versões aparecerão aqui após “Salvar nova versão”.",
                        );
                    }
                });
            },
        );
        if export {
            let body = self.editors[&id].body.clone();
            match crate::draft_tools::export(&export_capture, &body) {
                Ok(path) => {
                    self.message(format!("Cópia exportada: {}", path.display()));
                    if let Some(parent) = path.parent() {
                        crate::windows_ui::reveal_in_explorer(parent);
                    }
                }
                Err(e) => self.error(format!("Não foi possível exportar a minuta: {e}")),
            }
        }
        if let Some(draft) = compare {
            let body = self.editors[&id].body.clone();
            self.draft_tools.open_comparison(
                format!("Versão #{} • {}", draft.id, display_date(&draft.created_at)),
                draft.body,
                body,
            );
        }
        if let Some(uid) = material {
            self.material = Some((id, uid));
        }
        if template {
            let editor_kind = self.editors[&id].kind.clone();
            let chosen = self.editors[&id]
                .template_uid
                .as_ref()
                .and_then(|uid| offered.iter().find(|t| &t.uid == uid));
            let (body, origin) = match chosen.and_then(|t| t.published_version().map(|v| (t, v))) {
                Some((found, version)) => (
                    crate::prompting::skeleton(version, &detail.capture),
                    format!("Estrutura do modelo \"{}\" — revisar campos", found.name),
                ),
                None => (
                    ai::template_reply(&detail.capture, &editor_kind),
                    "Modelo sem IA — revisar campos".into(),
                ),
            };
            let provenance = chosen.map(|t| crate::model::DraftProvenance {
                template_uid: Some(t.uid.clone()),
                template_version: Some(t.current_version),
                template_name: t.name.clone(),
                ..Default::default()
            });
            self.replace_editor(Replacement {
                document_id: id,
                capture_id: detail.summary.latest_capture_id,
                kind: editor_kind,
                body,
                origin,
                saved: false,
                from_suggestion: false,
                materials: Vec::new(),
                provenance,
            });
        }
        if save_as_template {
            if self.templates.dirty() {
                self.error("Há um modelo com alterações não salvas na área Modelos. Salve ou descarte antes de criar outro.");
            } else {
                let (kind, body) = {
                    let editor = &self.editors[&id];
                    (editor.kind.clone(), editor.body.clone())
                };
                self.templates.start_from_text(&kind, &body);
                self.navigate(Leave::Templates);
            }
        }
        if let Some(intent) = generate {
            self.generate(detail, intent, ui.ctx());
        }
        if save {
            self.save_draft(id);
        }
        if let Some(draft) = loaded {
            self.replace_editor(Replacement {
                document_id: id,
                capture_id: draft.capture_id,
                kind: draft.kind,
                body: draft.body,
                origin: format!("Versão salva #{}", draft.id),
                saved: true,
                from_suggestion: false,
                materials: draft.materials,
                provenance: draft.provenance,
            });
        }
    }

    pub(super) fn suggestion_window(&mut self, ctx: &egui::Context) {
        if !self.suggestion_open {
            return;
        }
        let Some(suggestion) = &self.suggestion else {
            return;
        };
        let mut open = true;
        let mut use_text = false;
        let mut discard = false;
        egui::Window::new("Revisar sugestão de minuta")
            .open(&mut open)
            .default_size([720.0, 570.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = S2;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&suggestion.number)
                            .font(theme::font_strong(theme::SIZE_SECTION)),
                    );
                    pill(ui, kind_label(&suggestion.kind), Tone::Neutral);
                    pill(ui, "ainda não salva", Tone::Warn);
                });
                widgets::small_muted(
                    ui,
                    format!("Baseada na importação nº {}", suggestion.capture_id),
                );
                ui.label("Leia o texto e leve ao editor para corrigir e salvar uma versão.");
                widgets::card(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(380.0)
                        .show(ui, |ui| {
                            let mut text = suggestion.body.as_str();
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(20),
                            );
                        });
                });
                ui.horizontal(|ui| {
                    use_text = ui.add(widgets::primary("Levar ao editor")).clicked();
                    if ui
                        .add(widgets::secondary(format!(
                            "{} Copiar sugestão",
                            icon::COPY
                        )))
                        .clicked()
                    {
                        ui.ctx().copy_text(suggestion.body.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        discard = ui.add(widgets::danger("Descartar sugestão")).clicked();
                    });
                });
            });
        self.suggestion_open = open;
        if discard {
            self.suggestion = None;
            self.suggestion_open = false;
        }
        if use_text {
            if let Some(suggestion) = self.suggestion.clone() {
                // A revision is shown next to the text it replaces.
                if suggestion.intent == crate::work_templates::Intent::Revise {
                    self.draft_tools.open_comparison(
                        "Texto anterior".into(),
                        suggestion.previous_text.clone(),
                        suggestion.body.clone(),
                    );
                }
                self.replace_editor(Replacement {
                    document_id: suggestion.document_id,
                    capture_id: suggestion.capture_id,
                    kind: suggestion.kind,
                    body: suggestion.body,
                    origin: match suggestion.intent {
                        crate::work_templates::Intent::New => "Sugestão de IA revisável".into(),
                        crate::work_templates::Intent::Complete => {
                            "Texto completado pela IA — revisar".into()
                        }
                        crate::work_templates::Intent::Revise => {
                            "Texto revisado pela IA — comparar e revisar".into()
                        }
                    },
                    saved: false,
                    from_suggestion: true,
                    materials: suggestion.materials,
                    provenance: suggestion.provenance,
                });
                self.suggestion_open = false;
            }
        }
    }

    /// The window "Material utilizado": everything a generation used, as
    /// recorded at the time, so the text can be read with its basis.
    pub(super) fn material_window(&mut self, ctx: &egui::Context) {
        let Some((document_id, uid)) = self.material.clone() else {
            return;
        };
        let record = match self.store.generation_by_uid(document_id, &uid) {
            Ok(Some(record)) => record,
            Ok(None) => {
                self.material = None;
                self.error("O registro desta geração não está neste acervo.");
                return;
            }
            Err(error) => {
                self.material = None;
                self.error(format!("Não foi possível ler o registro: {error}"));
                return;
            }
        };
        let mut open = true;
        egui::Window::new("Material utilizado na geração")
            .open(&mut open)
            .default_size([700.0, 560.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = S3;
                    widgets::card(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = S2;
                        let template = match &record.template_uid {
                            Some(_) => format!(
                                "{} (versão {})",
                                record.template_name,
                                record.template_version.unwrap_or(0)
                            ),
                            None => "nenhum".to_owned(),
                        };
                        facts(
                            ui,
                            "material-facts",
                            &[
                                ("O que foi pedido", record.intent.label().to_owned()),
                                ("Modelo de trabalho", template),
                                ("IA", format!("{} ({})", record.model, record.provider)),
                                (
                                    "Feito",
                                    format!(
                                        "por {} em {}",
                                        record.created_by,
                                        display_date(&record.created_at)
                                    ),
                                ),
                            ],
                        );
                        match &record.outcome {
                            crate::work_templates::Outcome::Accepted => {
                                ui.horizontal_wrapped(|ui| {
                                    pill(ui, "Aceito", Tone::Ok);
                                    ui.label("Texto aceito pelo programa e levado à revisão.");
                                });
                            }
                            crate::work_templates::Outcome::Refused { reason } => {
                                notice_line(ui, Tone::Warn, format!("Resultado: recusado — {reason}"));
                            }
                        }
                    });
                    if !record.request_guidance.trim().is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Pedido desta minuta");
                            ui.add(egui::Label::new(&record.request_guidance).wrap());
                        });
                    }
                    if !record.blocks.is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Instruções aplicadas");
                            for block in &record.blocks {
                                ui.collapsing(
                                    format!("{} (versão {}, {})", block.name, block.version, block.mode),
                                    |ui| {
                                        ui.add(egui::Label::new(&block.text).wrap());
                                    },
                                );
                            }
                        });
                    }
                    if !record.example_uids.is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(
                                ui,
                                format!(
                                    "Exemplos aprovados mostrados: {}",
                                    record.example_uids.len()
                                ),
                            );
                            widgets::small_muted(ui, "Os exemplos mostram só a forma; seus fatos não valem para este caso.");
                        });
                    }
                    if !record.sources.is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Fontes selecionadas");
                            for source in &record.sources {
                                ui.add(
                                    egui::Label::new(format!("{} — {}", source.id, source.label))
                                        .wrap(),
                                );
                            }
                            widgets::small_muted(ui, "Um trecho citado foi encontrado literalmente na fonte; isso não comprova a conclusão.");
                        });
                    }
                    if !record.conflicts_shown.is_empty() {
                        widgets::card(ui, |ui| {
                            widgets::section(ui, "Avisos de conflito exibidos antes de gerar");
                            for conflict in &record.conflicts_shown {
                                notice_line(ui, Tone::Warn, conflict.as_str());
                            }
                        });
                    }
                    widgets::small_muted(
                        ui,
                        format!(
                            "Instruções enviadas: {} • dados: {} • código das instruções: {}",
                            crate::labels::file_size(record.system_bytes as u64),
                            crate::labels::file_size(record.data_bytes as u64),
                            record.system_sha256.chars().take(12).collect::<String>()
                        ),
                    );
                });
            });
        if !open {
            self.material = None;
        }
    }
}
