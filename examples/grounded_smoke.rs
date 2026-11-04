fn main() -> anyhow::Result<()> {
    let mut capture: ponteproj::model::Capture =
        serde_json::from_str(include_str!("../tests/fixtures/capture.json"))?;
    capture.text="Documento fictício recebido apenas para ciência e download. Não há solicitação de resposta nem prazo expresso.".into();
    let source = ponteproj::evidence::Source {
        id: "C1F1P1".into(),
        document_id: 1,
        capture_id: 1,
        label: "PDF fictício — página 1".into(),
        file_id: Some(1),
        page: Some(1),
        method: "pdf".into(),
        text: capture.text.clone(),
    };
    let settings = ponteproj::ai::AiSettings {
        base_url: "http://127.0.0.1:11434".into(),
        model: "qwen3:4b-instruct".into(),
        ..Default::default()
    };
    let analysis = ponteproj::ai::analyze_with_sources(
        &settings,
        &capture,
        1,
        "Finalidade confirmada: somente ciência e download.",
        &[source.clone()],
    )?;
    assert!(!analysis.citations.is_empty());
    assert!(analysis.deadline_candidate.is_none());
    let body=ponteproj::ai::generate_reply_with_sources(&settings,&capture,"resposta","Prepare somente um registro local de ciência e download para revisão, sem inventar atos no SIGADOC.",&[source])?;
    assert!(body.contains("BASES PARA REVISÃO"));
    println!(
        "Minuta fictícia com bases conferidas: {} bytes.",
        body.len()
    );
    println!(
        "Análise fictícia com {} citações literalmente conferidas e {} fontes preservadas.",
        analysis.citations.len(),
        analysis.materials.len()
    );
    Ok(())
}
