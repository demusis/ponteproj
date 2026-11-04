fn main() -> anyhow::Result<()> {
    let result = std::thread::spawn(|| {
        ponteproj::pdf_reader::render(
            include_bytes!("../tests/fixtures/document.pdf"),
            0,
            1400,
            true,
        )
    })
    .join()
    .unwrap()?;
    match result.ocr {
        Some(Ok(text)) => println!(
            "OCR local disponível: idioma {}, {} caracteres (PDF fictício)",
            text.language,
            text.text.len()
        ),
        Some(Err(error)) => println!("OCR opcional indisponível neste perfil: {error}"),
        None => anyhow::bail!("OCR não executado"),
    };
    Ok(())
}
