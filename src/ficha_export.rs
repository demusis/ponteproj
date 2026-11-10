//! A document's ficha as one self-contained HTML page, for printing from the
//! browser or keeping as a file. Pure text building: nothing here reads the
//! library or writes to disk.

use crate::labels;

/// One label/value row of "Dados do documento".
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub label: String,
    pub value: String,
}

/// A suggested action of the analysis (already in the person's words).
#[derive(Clone, Debug, Default)]
pub struct ActionLine {
    pub action: String,
    pub reason: String,
    pub evidence: String,
}

/// The analysis of the document, as the Resumo card shows it.
#[derive(Clone, Debug, Default)]
pub struct AnalysisBlock {
    pub summary: String,
    /// Priority key ("alta", "urgente"…); shown through `labels::priority`.
    pub priority: String,
    pub reason: String,
    pub deadline_candidate: Option<String>,
    pub subjects: Vec<String>,
    pub actions: Vec<ActionLine>,
    /// The analysis predates the latest changes.
    pub stale: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TaskLine {
    pub title: String,
    /// Status key ("pendente", "concluida"…).
    pub status: String,
    pub priority: String,
    /// Deadline, ISO date or empty.
    pub due_date: String,
    pub assignee: String,
}

#[derive(Clone, Debug, Default)]
pub struct AttendanceLine {
    pub reference: String,
    /// Already in the person's words.
    pub status: String,
    pub due_date: String,
    pub assignee: String,
}

#[derive(Clone, Debug, Default)]
pub struct FileLine {
    pub name: String,
    pub size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MovementLine {
    pub date: String,
    pub event: String,
    pub details: String,
}

/// Everything that goes on the page; the caller fills it from data the ficha
/// already has.
#[derive(Clone, Debug, Default)]
pub struct FichaPage {
    pub number: String,
    pub title: String,
    /// Kind and origin, as the ficha header shows them.
    pub origin_line: String,
    /// Document date as stored (ISO or free text).
    pub document_date: String,
    /// Import time (RFC 3339).
    pub imported_at: String,
    /// Typed in the Ponte, not imported.
    pub manual: bool,
    /// Priority key, or empty.
    pub priority: String,
    /// Task situation of the document in words, or empty.
    pub task_status: String,
    /// "dd/mm/aaaa hh:mm", already formatted by the caller.
    pub generated_at: String,
    pub rows: Vec<Row>,
    pub analysis: Option<AnalysisBlock>,
    pub tasks: Vec<TaskLine>,
    pub attendances: Vec<AttendanceLine>,
    pub note: String,
    pub text: String,
    /// Heading of the text section; "Texto do documento" when empty.
    pub text_title: String,
    pub files: Vec<FileLine>,
    pub history: Vec<MovementLine>,
    /// Current minuta text; only when the caller chooses to include it.
    pub minuta: Option<String>,
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn date_or_blank(value: &str) -> String {
    if value.trim().is_empty() {
        String::new()
    } else {
        labels::date(value)
    }
}

const STYLE: &str = "\
:root{color-scheme:light}\
*{box-sizing:border-box}\
body{margin:0;background:#fff;color:#000;font-family:'Segoe UI',system-ui,-apple-system,Roboto,'Helvetica Neue',Arial,sans-serif;font-size:11pt;line-height:1.45}\
main{max-width:46rem;margin:0 auto;padding:2rem 1.25rem 3rem}\
h1{font-size:1.5rem;line-height:1.25;margin:.2rem 0 .4rem}\
h2{font-size:1.1rem;margin:1.8rem 0 .5rem;padding-bottom:.25rem;border-bottom:1.5px solid #000;break-after:avoid}\
h3{font-size:1rem;margin:1rem 0 .3rem;break-after:avoid}\
.num{font-weight:600;letter-spacing:.02em}\
.meta{margin:.15rem 0;color:#333}\
.gen{margin-top:.6rem;font-size:.85rem;color:#444}\
table{width:100%;border-collapse:collapse}\
th,td{text-align:left;vertical-align:top;padding:.3rem .5rem;border-bottom:1px solid #bbb}\
th{font-weight:600;width:11rem;color:#222}\
table.grid th{width:auto;border-bottom:1.5px solid #000}\
tr,.card{break-inside:avoid}\
.card{border:1px solid #888;border-radius:4px;padding:.5rem .75rem;margin:.5rem 0}\
.note{border-left:4px solid #000;padding:.3rem .75rem;margin:.5rem 0;font-size:.95rem}\
.done{text-decoration:line-through;color:#444}\
.text{white-space:pre-wrap;overflow-wrap:anywhere;font-family:Consolas,'Courier New',monospace;font-size:10pt}\
.pre{white-space:pre-wrap;overflow-wrap:anywhere}\
ul{margin:.3rem 0;padding-left:1.3rem}\
@page{margin:18mm 16mm}\
@media print{main{max-width:none;padding:0}a{color:#000}}";

/// The whole page, a complete standalone HTML document.
pub fn html(page: &FichaPage) -> String {
    let mut o = String::new();
    let heading = format!("{} {}", page.number, page.title);
    o.push_str("<!doctype html>\n<html lang=\"pt-BR\">\n<head>\n<meta charset=\"utf-8\">\n");
    o.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    o.push_str(&format!("<title>{}</title>\n", esc(heading.trim())));
    o.push_str(&format!(
        "<style>{STYLE}</style>\n</head>\n<body>\n<main>\n"
    ));

    // Header.
    o.push_str("<header>\n");
    o.push_str(&format!("<div class=\"num\">{}</div>\n", esc(&page.number)));
    o.push_str(&format!("<h1>{}</h1>\n", esc(&page.title)));
    if !page.origin_line.trim().is_empty() {
        o.push_str(&format!(
            "<p class=\"meta\">{}</p>\n",
            esc(&page.origin_line)
        ));
    }
    let mut dates = Vec::new();
    if !page.document_date.trim().is_empty() {
        dates.push(format!(
            "Data do documento: {}",
            esc(&date_or_blank(&page.document_date))
        ));
    }
    if !page.imported_at.trim().is_empty() {
        dates.push(format!(
            "{}: {}",
            if page.manual {
                "Registrado em"
            } else {
                "Importado em"
            },
            esc(&labels::datetime(&page.imported_at))
        ));
    }
    if !dates.is_empty() {
        o.push_str(&format!("<p class=\"meta\">{}</p>\n", dates.join(" · ")));
    }
    let mut state = Vec::new();
    if !page.priority.trim().is_empty() {
        state.push(format!(
            "Prioridade: {}",
            esc(&labels::priority(&page.priority).to_lowercase())
        ));
    }
    if !page.task_status.trim().is_empty() {
        state.push(format!("Tarefas: {}", esc(&page.task_status)));
    }
    if !state.is_empty() {
        o.push_str(&format!("<p class=\"meta\">{}</p>\n", state.join(" · ")));
    }
    if !page.generated_at.trim().is_empty() {
        o.push_str(&format!(
            "<p class=\"gen\">Gerado pelo Ponte em {}</p>\n",
            esc(&page.generated_at)
        ));
    }
    o.push_str("</header>\n");

    if !page.rows.is_empty() {
        o.push_str("<section>\n<h2>Dados do documento</h2>\n<table>\n");
        for row in &page.rows {
            o.push_str(&format!(
                "<tr><th scope=\"row\">{}</th><td>{}</td></tr>\n",
                esc(&row.label),
                esc(&row.value)
            ));
        }
        o.push_str("</table>\n</section>\n");
    }

    if let Some(a) = &page.analysis {
        o.push_str("<section>\n<h2>Análise</h2>\n");
        o.push_str("<p class=\"meta\"><strong>Análise feita com apoio de IA. Confira na fonte.</strong></p>\n");
        if a.stale {
            o.push_str("<p class=\"note\">Esta análise é anterior às últimas alterações do documento, do contexto ou das notas.</p>\n");
        }
        if !a.summary.trim().is_empty() {
            o.push_str(&format!(
                "<p class=\"pre\">{}</p>\n",
                esc(a.summary.trim_end())
            ));
        }
        o.push_str("<table>\n");
        if !a.priority.trim().is_empty() {
            o.push_str(&format!(
                "<tr><th scope=\"row\">Prioridade sugerida</th><td>{}</td></tr>\n",
                esc(labels::priority(&a.priority))
            ));
        }
        if !a.reason.trim().is_empty() {
            o.push_str(&format!(
                "<tr><th scope=\"row\">Motivo</th><td>{}</td></tr>\n",
                esc(&a.reason)
            ));
        }
        if let Some(deadline) = a
            .deadline_candidate
            .as_ref()
            .filter(|d| !d.trim().is_empty())
        {
            o.push_str(&format!(
                "<tr><th scope=\"row\">Prazo possível</th><td>{} (confirme a data e a regra na fonte)</td></tr>\n",
                esc(deadline)
            ));
        }
        if !a.subjects.is_empty() {
            o.push_str(&format!(
                "<tr><th scope=\"row\">Assuntos</th><td>{}</td></tr>\n",
                esc(&a.subjects.join(" • "))
            ));
        }
        o.push_str("</table>\n");
        if !a.actions.is_empty() {
            o.push_str("<h3>Providências sugeridas</h3>\n");
            for action in &a.actions {
                o.push_str("<div class=\"card\">");
                o.push_str(&format!("<strong>{}</strong>", esc(&action.action)));
                if !action.reason.trim().is_empty() {
                    o.push_str(&format!("<div>{}</div>", esc(&action.reason)));
                }
                if !action.evidence.trim().is_empty() {
                    o.push_str(&format!(
                        "<div class=\"meta\">Base indicada: {}</div>",
                        esc(&action.evidence)
                    ));
                }
                o.push_str("</div>\n");
            }
        }
        o.push_str("</section>\n");
    }

    if !page.tasks.is_empty() {
        o.push_str("<section>\n<h2>Tarefas</h2>\n<table class=\"grid\">\n<tr><th>Tarefa</th><th>Situação</th><th>Prioridade</th><th>Prazo</th><th>Responsável</th></tr>\n");
        for t in &page.tasks {
            let done = t.status == "concluida";
            let title = if done {
                format!("<span class=\"done\">{}</span>", esc(&t.title))
            } else {
                esc(&t.title)
            };
            o.push_str(&format!(
                "<tr><td>{}</td><td>{}{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                title,
                esc(labels::task_status(&t.status)),
                if done { " ✓" } else { "" },
                if t.priority.trim().is_empty() {
                    String::new()
                } else {
                    esc(labels::priority(&t.priority))
                },
                esc(&date_or_blank(&t.due_date)),
                esc(&t.assignee)
            ));
        }
        o.push_str("</table>\n</section>\n");
    }

    if !page.attendances.is_empty() {
        let title = if page.attendances.len() == 1 {
            "Atendimento"
        } else {
            "Atendimentos"
        };
        o.push_str(&format!("<section>\n<h2>{title}</h2>\n<table class=\"grid\">\n<tr><th>Referência</th><th>Situação</th><th>Prazo</th><th>Responsável</th></tr>\n"));
        for a in &page.attendances {
            o.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                esc(&a.reference),
                esc(&a.status),
                esc(&date_or_blank(&a.due_date)),
                esc(&a.assignee)
            ));
        }
        o.push_str("</table>\n</section>\n");
    }

    if !page.note.trim().is_empty() {
        o.push_str(&format!(
            "<section>\n<h2>Nota</h2>\n<p class=\"pre\">{}</p>\n</section>\n",
            esc(page.note.trim_end())
        ));
    }

    if !page.text.trim().is_empty() {
        let title = if page.text_title.trim().is_empty() {
            "Texto do documento"
        } else {
            page.text_title.as_str()
        };
        o.push_str(&format!(
            "<section>\n<h2>{}</h2>\n<div class=\"text\">{}</div>\n</section>\n",
            esc(title),
            esc(page.text.trim_end())
        ));
    }

    if let Some(minuta) = page.minuta.as_ref().filter(|m| !m.trim().is_empty()) {
        o.push_str(&format!(
            "<section>\n<h2>Minuta</h2>\n<div class=\"text\">{}</div>\n</section>\n",
            esc(minuta.trim_end())
        ));
    }

    if !page.files.is_empty() {
        o.push_str("<section>\n<h2>Arquivos e anexos</h2>\n<ul>\n");
        for f in &page.files {
            o.push_str(&format!(
                "<li>{} ({})</li>\n",
                esc(&f.name),
                esc(&labels::file_size(f.size))
            ));
        }
        o.push_str("</ul>\n<p class=\"meta\">Os arquivos não estão nesta página; ficam no Ponte.</p>\n</section>\n");
    }

    if !page.history.is_empty() {
        o.push_str("<section>\n<h2>Histórico de tramitação</h2>\n<table class=\"grid\">\n<tr><th>Data</th><th>Evento</th><th>Detalhes</th></tr>\n");
        for m in &page.history {
            o.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>\n",
                esc(&m.date),
                esc(&m.event),
                esc(&m.details)
            ));
        }
        o.push_str("</table>\n</section>\n");
    }

    o.push_str("</main>\n</body>\n</html>\n");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FichaPage {
        FichaPage {
            number: "ORG-OFI-2026/00431".into(),
            title: "Solicitação de informações".into(),
            origin_line: "Importado do SIGADOC · importação nº 2".into(),
            document_date: "2026-03-10".into(),
            imported_at: "2026-03-12T14:30:00-04:00".into(),
            manual: false,
            priority: "alta".into(),
            task_status: "1 pendente".into(),
            generated_at: "10/11/2026 09:15".into(),
            rows: vec![Row {
                label: "Tipo".into(),
                value: "Ofício".into(),
            }],
            analysis: Some(AnalysisBlock {
                summary: "Pede informações sobre o contrato.".into(),
                priority: "alta".into(),
                reason: "Há prazo curto.".into(),
                deadline_candidate: Some("2026-03-20".into()),
                subjects: vec!["Contratos".into()],
                actions: vec![ActionLine {
                    action: "Responder".into(),
                    reason: "Pedido direto.".into(),
                    evidence: "Parágrafo 2".into(),
                }],
                stale: true,
            }),
            tasks: vec![
                TaskLine {
                    title: "Responder ao ofício".into(),
                    status: "pendente".into(),
                    priority: "alta".into(),
                    due_date: "2026-03-20".into(),
                    assignee: "Servidor A".into(),
                },
                TaskLine {
                    title: "Triar documento".into(),
                    status: "concluida".into(),
                    ..Default::default()
                },
            ],
            attendances: vec![AttendanceLine {
                reference: "ORG-ATD-2026/0007".into(),
                status: "Em andamento".into(),
                due_date: "2026-03-25".into(),
                assignee: "Servidor B".into(),
            }],
            note: "Primeira linha\nSegunda linha".into(),
            text: "Texto do ofício.\nSegundo parágrafo.".into(),
            text_title: "Texto importado".into(),
            files: vec![FileLine {
                name: "oficio.pdf".into(),
                size: 245_760,
            }],
            history: vec![MovementLine {
                date: "10/03/2026".into(),
                event: "Recebimento".into(),
                details: "Unidade A · Servidor A".into(),
            }],
            minuta: None,
        }
    }

    #[test]
    fn untrusted_text_is_escaped() {
        let page = FichaPage {
            title: "<script>alert(\"x\")</script> & 'y'".into(),
            text: "<img src=x onerror=alert(1)>".into(),
            ..sample()
        };
        let out = html(&page);
        assert!(!out.contains("<script"));
        assert!(!out.contains("<img"));
        assert!(out.contains("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;y&#39;"));
    }

    #[test]
    fn empty_sections_are_left_out() {
        let page = FichaPage {
            number: "ORG-OFI-2026/00001".into(),
            title: "Vazio".into(),
            ..Default::default()
        };
        let out = html(&page);
        for h in [
            "Dados do documento",
            "Análise",
            "Tarefas",
            "Atendimento",
            "Nota",
            "Texto do documento",
            "Arquivos e anexos",
            "Histórico de tramitação",
            "Minuta",
        ] {
            assert!(!out.contains(&format!("<h2>{h}")), "{h}");
        }
    }

    #[test]
    fn line_breaks_are_kept() {
        let out = html(&sample());
        assert!(out.contains("Primeira linha\nSegunda linha"));
        assert!(out.contains("white-space:pre-wrap"));
    }

    #[test]
    fn stale_note_only_when_stale() {
        let stale = html(&sample());
        assert!(stale.contains("Esta análise é anterior às últimas alterações"));
        assert!(stale.contains("Análise feita com apoio de IA. Confira na fonte."));
        let mut page = sample();
        page.analysis.as_mut().unwrap().stale = false;
        assert!(!html(&page).contains("anterior às últimas"));
    }

    #[test]
    fn minuta_only_when_given() {
        assert!(!html(&sample()).contains("<h2>Minuta"));
        let page = FichaPage {
            minuta: Some("Prezados,\nSegue.".into()),
            ..sample()
        };
        assert!(html(&page).contains("<h2>Minuta"));
    }

    #[test]
    fn page_is_well_formed() {
        let out = html(&sample());
        assert!(out.starts_with("<!doctype html>"));
        assert!(out.contains("lang=\"pt-BR\""));
        assert!(out.contains("<title>ORG-OFI-2026/00431 Solicitação de informações</title>"));
        for tag in ["html", "head", "body", "main", "section", "table"] {
            assert_eq!(
                out.matches(&format!("<{tag}>")).count() + out.matches(&format!("<{tag} ")).count(),
                out.matches(&format!("</{tag}>")).count(),
                "{tag}"
            );
        }
        assert_eq!(out.matches("<html").count(), 1);
        assert_eq!(out.matches("<body").count(), 1);
        assert!(!out.contains("http://") && !out.contains("https://"));
        assert!(!out.contains("<link") && !out.contains("src="));
    }

    /// Prints the sample page, for inspecting it by eye (`--nocapture`).
    #[test]
    fn sample_page_renders() {
        let out = html(&sample());
        println!("{out}");
        assert!(out.contains("Gerado pelo Ponte em 10/11/2026 09:15"));
        assert!(out.contains("240 KB"));
    }
}
