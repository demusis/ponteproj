//! A test library on this computer for the phone prototype, kept apart from
//! the real one: it plays the desktop that shares `ponte-sync.ponte`.
//!
//!   acervo_teste criar <pasta-do-acervo> <arquivo.ponte> <frase-senha>
//!       creates the library with the sample document and writes the first
//!       transport copy (put the file in a test folder of Google Drive);
//!   acervo_teste ver <pasta-do-acervo> <arquivo.ponte> <frase-senha>
//!       synchronizes with the file and lists the documents and their
//!       notes (what the phone wrote shows up here).
use anyhow::{bail, Context, Result};
use ponteproj::intranet::{IntranetConfig, IntranetMode};
use ponteproj::model::Capture;
use ponteproj::storage::Store;
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [command, dir, file, passphrase] = args.as_slice() else {
        bail!("Uso: acervo_teste criar|ver <pasta-do-acervo> <arquivo.ponte> <frase-senha>");
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir)?;
    // Everything of this test library stays in its own folder.
    std::env::set_var("PONTEPROJ_DATA_DIR", &dir);
    let mut store = Store::open(&dir.join("acervo.sqlite3"))?;
    let reporter = ponteproj::ai::Reporter::silent();
    match command.as_str() {
        "criar" => {
            let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/capture.json");
            let capture: Capture = serde_json::from_slice(
                &std::fs::read(fixture).context("Captura de exemplo não encontrada.")?,
            )?;
            if store.list_documents("")?.is_empty() {
                store.import_capture(&capture)?;
            }
            let cfg = IntranetConfig {
                mode: IntranetMode::SharedFolder,
                member_name: "Computador de teste".into(),
                folder_files: vec![file.clone()],
                remember_folder_key: false,
                folder_writer: uuid::Uuid::new_v4().simple().to_string(),
                ..store.load_intranet_config()?
            };
            store.save_intranet_config(&cfg)?;
            let report = store.sync_with_folders(&cfg, passphrase, false, &reporter)?;
            println!("Criado: {}", report.summary());
        }
        "ver" => {
            let cfg = store.load_intranet_config()?;
            let report = store.sync_with_folders(&cfg, passphrase, false, &reporter)?;
            println!("Sincronizado: {}", report.summary());
            if report.conflicts > 0 {
                println!("Conflitos: {}", report.conflicts);
            }
        }
        other => bail!("Comando desconhecido: {other}"),
    }
    for summary in store.list_documents("")? {
        let note = store.load_document(summary.id)?.note;
        println!("- {} | {}", summary.document_number, summary.title);
        println!("  nota: {}", if note.is_empty() { "(vazia)" } else { note.as_str() });
    }
    Ok(())
}
