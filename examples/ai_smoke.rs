//! Explicit live smoke check using only synthetic data and no persistence.
//! Run: cargo run --example ai_smoke -- qwen3:4b-instruct
use anyhow::{ensure, Result};
use ponteproj::ai::{analyze_capture, generate_reply, AiSettings};
use ponteproj::model::{Capture, Coverage};
use std::collections::BTreeMap;

fn main() -> Result<()> {
    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "qwen3:4b-instruct".into());
    let settings = AiSettings {
        base_url: "http://127.0.0.1:11434".into(),
        model,
        ..Default::default()
    };
    let capture = Capture {
        schema_version: 1,
        request_id: uuid::Uuid::new_v4().to_string(),
        source_url: "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=TESTE-PRO-2026%2F99999".into(),
        document_number: "TESTE-PRO-2026/99999".into(),
        title: "TESTE FICTÍCIO — reposição de papel".into(),
        captured_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        text: "DADO EXCLUSIVAMENTE SINTÉTICO PARA TESTE. Solicita-se reposição de papel de escritório. Não há destinatário, valor, prazo informado ou providência já concluída. Não corresponde a documento real.".into(),
        metadata: BTreeMap::new(),
        references: vec![],
        coverage: Coverage {
            adapter: "synthetic-ai-smoke-v1".into(),
            recognized: vec!["identidade fictícia".into(), "texto sintético".into()],
            pending: vec!["Este teste não possui anexos".into()],
        },
    };
    // Synthetic capture ID; the example deliberately never opens a Store.
    let analysis = analyze_capture(&settings, &capture, 1)?;
    ensure!(
        analysis.capture_id == 1,
        "A análise perdeu o vínculo com a captura sintética."
    );
    ensure!(
        analysis.deadline_candidate.is_none(),
        "O modelo sugeriu prazo ausente do texto sintético; revise o comportamento antes de usar."
    );
    let reply = generate_reply(&settings, &capture, "resposta", "Não presumir destinatário ou atendimento. Deixar campos desconhecidos para preencher na revisão.")?;
    ensure!(
        reply.contains(&capture.document_number),
        "A minuta não citou o número sintético solicitado."
    );
    println!("OK: Ollama local, análise JSON validada e minuta recebida; prioridade={}, análise={} bytes, minuta={} bytes. Nenhum dado foi persistido.", analysis.priority, analysis.summary.len(), reply.len());
    Ok(())
}
