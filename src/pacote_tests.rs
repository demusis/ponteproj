//! Pacote F2 + F3 (0.52.0): the features of 0.51.0 that create or edit
//! records, under the rules of F3 (identity given at creation, `refs` on
//! every change, an edit only on the record it names). Tests only.
#![allow(clippy::all)]

use crate::intranet::{IntranetConfig, IntranetMode, IntranetMutation};
use crate::model::ImportOutcome;
use crate::storage::Store;

const KEY: &str = "senha-de-teste-123";

fn folder_config(member: &str, files: Vec<String>) -> IntranetConfig {
    IntranetConfig {
        mode: IntranetMode::SharedFolder,
        member_name: member.into(),
        folder_files: files,
        folder_key: KEY.into(),
        ..IntranetConfig::default()
    }
}

/// The product always may move a folder to format 9; the tests of the old
/// exchange keep it off, these turn it on (as the tests of F2b and F3 do).
fn on() {
    crate::idents::hooks::PROMOTION.with(|c| c.set(true));
}

fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
    let cfg = store.load_intranet_config().unwrap();
    store
        .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
        .unwrap()
}

fn ident(store: &Store, row: i64) -> Option<String> {
    crate::idents::ident_of(&store.conn, "document", row).unwrap()
}

fn row_of(store: &Store, ident: &str) -> Option<i64> {
    use rusqlite::OptionalExtension;
    store
        .conn
        .query_row(
            "SELECT row_id FROM item_ident WHERE kind='document' AND ident=?1",
            [ident],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
}

/// A conversation as the phone exports it (Android, "Exportar conversa ▸
/// Sem mídia"): a BOM, CRLF line ends and a message over several lines.
const ANDROID_EXPORT: &str = "\u{feff}12/11/2026 09:20 - Ana Souza: Bom dia, pessoal!\r\n\
12/11/2026 09:21 - Bruno Lima: Segue a pauta:\r\n\
- item 1\r\n\
- item 2\r\n\
12/11/2026 14:05 - Bruno Lima: <Mídia oculta>\r\n";

/// 0.51.0's "Importar arquivo de conversa (.txt)…" goes through the same
/// creation as a paste (`Store::import_whatsapp`), which F3 already makes a
/// producer of proofs: the record gets its identity when it is created, the
/// queued creation carries `refs: Some` with no site (a creation names no
/// existing item), the other device receives it under the same identity,
/// the same file imported again is the same record, and an edit made on the
/// other device names the record by identity (`manual_target`) and lands on
/// it.
#[test]
fn whatsapp_file_import_creates_with_identity_and_proofs_and_edits_land_on_it() {
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let files = vec![drive.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", files.clone()))
        .unwrap();
    a.import_capture(&crate::model::sample_capture()).unwrap();
    sync(&mut a);
    assert!(sync(&mut a).promoted, "the folder moves to format 9");
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", files))
        .unwrap();
    sync(&mut b);

    // The file, read as the window reads it.
    let text = crate::whatsapp::decode_export(ANDROID_EXPORT.as_bytes()).unwrap();
    let conversation = crate::whatsapp::parse(&text).unwrap();
    assert_eq!(conversation.messages.len(), 3);
    let chat = crate::whatsapp::chat_from_file_name("Conversa do WhatsApp com Equipe B.txt");
    assert_eq!(chat, "Equipe B");
    let title = crate::whatsapp::suggested_title(&chat, &conversation);
    let receipt = a
        .import_whatsapp(&chat, &title, &conversation, "whatsapp-arquivo")
        .unwrap();
    assert_eq!(receipt.effective_outcome(), ImportOutcome::Imported);
    let on_a = receipt.document_id;
    let doc_ident = ident(&a, on_a).expect("the record has its identity at creation");

    // The queued creation is an F3 change: refs present, no site, and the
    // identity of the row it creates travels with it.
    let outbox = a.list_offline_outbox().unwrap();
    let entry = outbox
        .iter()
        .find(|e| matches!(&e.mutation, IntranetMutation::ImportCapture { capture } if capture.title == title))
        .expect("the creation is queued");
    let refs = entry.refs.as_ref().expect("an F3 producer writes refs");
    assert!(refs.mutation.is_empty() && refs.base.is_empty());
    assert!(refs.manual_target.is_none(), "a creation targets nothing");
    assert!(
        entry.idents.iter().any(|s| s.ident == doc_ident),
        "the creation carries the record's identity: {:?}",
        entry.idents
    );

    // The other device receives it under the same identity.
    sync(&mut a);
    sync(&mut b);
    let on_b = row_of(&b, &doc_ident).expect("B has the record by its identity");
    let there = b.load_document(on_b).unwrap();
    assert_eq!(there.capture.title, title);
    let sent = a.load_document(on_a).unwrap().capture;
    assert_eq!(there.capture.text, sent.text);
    assert!(
        there.capture.text.contains("- item 2"),
        "{}",
        there.capture.text
    );

    // The same file again, on B: the same record, nothing new.
    let again = b
        .import_whatsapp(&chat, &title, &conversation, "whatsapp-arquivo")
        .unwrap();
    assert_ne!(again.effective_outcome(), ImportOutcome::Imported);
    assert_eq!(again.document_id, on_b);

    // An edit on B names the record by identity and lands on it on A.
    let mut fields = crate::manual::ManualFields::from_capture(&there.capture);
    fields
        .text
        .push_str("\nObservação acrescentada no outro computador.");
    b.update_manual_document(on_b, &fields).unwrap();
    let edit = b
        .list_offline_outbox()
        .unwrap()
        .into_iter()
        .find(|e| matches!(&e.mutation, IntranetMutation::ImportCapture { capture } if capture.text == fields.text))
        .expect("the edit is queued");
    let target = edit
        .refs
        .as_ref()
        .and_then(|r| r.manual_target.clone())
        .expect("the edit names its record");
    assert_eq!(
        target.proof,
        crate::idrefs::Proof::Ident(doc_ident.clone()),
        "by identity, never by number"
    );
    sync(&mut b);
    let report = sync(&mut a);
    assert_eq!(report.conflicts, 0, "{report:?}");
    assert_eq!(row_of(&a, &doc_ident), Some(on_a));
    let here = a.load_document(on_a).unwrap();
    assert!(
        here.capture
            .text
            .contains("Observação acrescentada no outro computador."),
        "the edit is on its own record"
    );
    assert!(a.list_sync_conflicts().unwrap().is_empty());
}

/// 0.51.0's size limit of the shared file, in format 9 (the file
/// `ponte-sync-formato9.ponte` beside the stub): a library that no longer
/// fits is refused before anything is written, by the bound of its blobs and
/// then by the exact size of a file of format 9 (whose header carries the
/// level), and neither the file, nor the stub, nor any copy beside them
/// changes; with room again the same change goes through.
#[test]
fn a_library_over_the_limit_is_refused_in_format_9_and_nothing_is_lost() {
    use crate::sharedfolder;
    on();
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let mut store = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    store
        .save_intranet_config(&folder_config("Casa", vec![drive.display().to_string()]))
        .unwrap();
    let document = store
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    sync(&mut store);
    assert!(sync(&mut store).promoted);
    let nine = sharedfolder::nine_path(&drive);
    let header = sharedfolder::read_header(&nine).unwrap().unwrap();
    assert_eq!(
        (header.format, header.level),
        (sharedfolder::FORMAT_9, sharedfolder::LEVEL)
    );
    let folder = drive.parent().unwrap();
    let listing = || {
        let mut names: Vec<String> = folder
            .read_dir()
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    };
    let (stub_before, nine_before, names_before) = (
        std::fs::read(&drive).unwrap(),
        std::fs::read(&nine).unwrap(),
        listing(),
    );
    let cfg = store.load_intranet_config().unwrap();
    let try_sync = |store: &mut Store| {
        store.sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
    };
    let unchanged = |store: &Store| {
        assert_eq!(std::fs::read(&drive).unwrap(), stub_before, "the stub");
        assert_eq!(
            std::fs::read(&nine).unwrap(),
            nine_before,
            "the file of format 9"
        );
        assert_eq!(listing(), names_before, "no copy kept aside, no leftover");
        assert!(
            !store.list_offline_outbox().unwrap().is_empty(),
            "still queued"
        );
    };

    let owner = crate::attachments::Owner::Document(document);
    store
        .add_attachment(owner, "grande.txt", "", &vec![b'x'; 6000])
        .unwrap();
    sharedfolder::set_write_limit_for_test(2000);
    let error = try_sync(&mut store).unwrap_err().to_string();
    assert!(
        error.starts_with("Nenhuma pasta pôde ser gravada: O acervo passa de 512 MB"),
        "{error}"
    );
    assert!(error.contains("Nada foi gravado"), "{error}");
    unchanged(&store);
    assert_eq!(store.list_attachments(owner).unwrap().len(), 1);

    // The blobs fit, the whole file of format 9 does not: refused by its
    // exact size, before any folder is touched.
    let bound = crate::folder_health::blob_bound(&store.export_snapshot().unwrap());
    sharedfolder::set_write_limit_for_test(bound + 1);
    let error = try_sync(&mut store).unwrap_err().to_string();
    assert!(
        error.contains("O acervo passa de 512 MB") && error.contains("daria"),
        "{error}"
    );
    unchanged(&store);

    sharedfolder::set_write_limit_for_test(sharedfolder::MAX_FILE_BYTES);
    let report = try_sync(&mut store).unwrap();
    assert!(!report.written_to.is_empty(), "{report:?}");
    let after = sharedfolder::read_header(&nine).unwrap().unwrap();
    assert!(after.revision > header.revision);
    assert_eq!(after.level, sharedfolder::LEVEL);
    assert_eq!(
        std::fs::read(&drive).unwrap(),
        stub_before,
        "the stub stays"
    );
    assert!(store.list_offline_outbox().unwrap().is_empty());
}

// ------------------------------------------------- revisão do pacote

/// Two computers on a folder still at format 8 (the promotion off while
/// they join): A made the first copy with one record, B took it.
struct AtEight {
    drive: std::path::PathBuf,
    a: Store,
    b: Store,
    document: i64,
    _dir: tempfile::TempDir,
}

fn at_eight() -> AtEight {
    crate::idents::hooks::PROMOTION.with(|c| c.set(false));
    let dir = tempfile::tempdir().unwrap();
    let drive = dir.path().join("Drive").join("ponte-sync.ponte");
    std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
    let files = vec![drive.display().to_string()];
    let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
    a.save_intranet_config(&folder_config("Casa", files.clone()))
        .unwrap();
    let document = a
        .import_capture(&crate::model::sample_capture())
        .unwrap()
        .document_id;
    sync(&mut a);
    let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
    b.save_intranet_config(&folder_config("Trabalho", files))
        .unwrap();
    sync(&mut b);
    assert_eq!(
        b.load_document(document).unwrap().capture.title,
        crate::model::sample_capture().title
    );
    let header = crate::sharedfolder::read_header(&drive).unwrap().unwrap();
    assert_eq!(header.format, crate::sharedfolder::FORMAT);
    AtEight {
        drive,
        a,
        b,
        document,
        _dir: dir,
    }
}

fn try_sync(store: &mut Store) -> anyhow::Result<crate::intranet::FolderSyncReport> {
    let cfg = store.load_intranet_config().unwrap();
    store.sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
}

/// Revisão do pacote, achado 2: the card of a folder at format 9 reads the
/// file that holds the library (`ponte-sync-formato9.ponte`), never the stub
/// at the old name, which keeps the revision of the promotion: a folder in
/// step after a synced change is never shown as behind this computer.
#[test]
fn a_healthy_folder_after_a_synced_change_is_never_shown_as_behind() {
    use crate::sharedfolder;
    let AtEight {
        drive,
        mut a,
        mut b,
        document,
        ..
    } = at_eight();
    on();
    assert!(sync(&mut a).promoted);
    sync(&mut b);
    let stub = sharedfolder::read_header(&drive).unwrap().unwrap();
    assert!(!stub.points_to.is_empty(), "the old name holds the stub");
    for (who, store) in [("A", &mut a), ("B", &mut b)] {
        store
            .save_note(document, &format!("Nota escrita em {who}"))
            .unwrap();
        sync(store);
    }
    sync(&mut a);
    for (who, store) in [("A", &a), ("B", &b)] {
        let saved = store.load_intranet_config().unwrap().synced_revision;
        assert!(saved > stub.revision, "{who}: the stub stays behind");
        let header = crate::folder_health::library_header(&drive)
            .unwrap()
            .unwrap();
        assert!(
            header.revision >= saved,
            "{who}: the card describes the library ({} < {saved})",
            header.revision
        );
        let (pill, tone, _) = crate::intranet::folder_pill(&header, false, saved);
        assert_eq!(pill, "Em dia", "{who}");
        assert_eq!(tone, crate::widgets::Tone::Ok, "{who}");
    }
    let header = crate::folder_health::library_header(&drive)
        .unwrap()
        .unwrap();
    assert_eq!(
        header.revision,
        a.load_intranet_config().unwrap().synced_revision
    );
}

/// Revisão do pacote, achado 4: a library that fits in a file of format 8
/// and not in one of format 9 does not stop syncing. The promotion waits
/// (the line says why and what to do), the folder stays at format 8 with
/// nothing of format 9 written and the floor where it was, this computer's
/// changes are sent in format 8 and the other computer's are received; with
/// room, the promotion goes on.
#[test]
fn a_library_that_fits_in_format_8_but_not_in_9_keeps_syncing_in_8() {
    use crate::sharedfolder;
    let AtEight {
        drive,
        mut a,
        mut b,
        document,
        ..
    } = at_eight();
    // Rows enough that the identities of format 9 weigh more than a note.
    for n in 0..40 {
        let mut capture = crate::model::sample_capture();
        capture.source_url = format!(
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=SEFAZ-PRO-2026%2F{:05}",
            100 + n
        );
        capture.document_number = format!("SEFAZ-PRO-2026/{:05}", 100 + n);
        capture.request_id = uuid::Uuid::new_v4().to_string();
        capture.title = format!("Registro {n}");
        a.import_capture(&capture).unwrap();
    }
    sync(&mut a);
    sync(&mut b);
    b.save_note(document, "Nota escrita em B").unwrap();
    sync(&mut b);
    // A in step with the folder (what the promotion asks), then a change
    // of its own.
    sync(&mut a);
    assert_eq!(a.load_document(document).unwrap().note, "Nota escrita em B");
    on();
    let limit = |extra: u64| {
        sharedfolder::set_write_limit_for_test(std::fs::metadata(&drive).unwrap().len() + extra)
    };
    let lineage = a.load_intranet_config().unwrap().folder_lineage;
    let still_eight = |a: &Store| {
        assert!(
            !sharedfolder::nine_path(&drive).exists(),
            "no file of format 9"
        );
        let header = sharedfolder::read_header(&drive).unwrap().unwrap();
        assert_eq!(header.format, sharedfolder::FORMAT);
        assert!(a.floor(&lineage).unwrap().format < sharedfolder::FORMAT_9);
    };
    let waits = |report: &crate::intranet::FolderSyncReport| {
        assert!(!report.promoted, "{report:?}");
        let wait = report.promotion_wait.clone().unwrap_or_default();
        assert!(wait.contains("não cabe no arquivo novo"), "{wait}");
        assert!(wait.contains("esvazie a Lixeira"), "{wait}");
        assert!(
            wait.contains("continua sincronizando no formato anterior"),
            "{wait}"
        );
        assert!(report.summary().contains(&wait));
    };
    // Nothing to send: the exchange ends well, in format 8.
    limit(16);
    let report = try_sync(&mut a);
    sharedfolder::set_write_limit_for_test(sharedfolder::MAX_FILE_BYTES);
    waits(&report.expect("A keeps syncing in format 8"));
    still_eight(&a);
    // A change of A that fits in format 8 is sent in format 8.
    a.save_note(document, "Nota de A").unwrap();
    limit(1500);
    let report = try_sync(&mut a);
    sharedfolder::set_write_limit_for_test(sharedfolder::MAX_FILE_BYTES);
    let report = report.expect("A sends in format 8");
    waits(&report);
    assert!(!report.written_to.is_empty(), "{report:?}");
    assert!(a.list_offline_outbox().unwrap().is_empty());
    still_eight(&a);
    // B is a computer of this version too, with the same library: it does
    // not fit either, receives and sends in format 8.
    limit(1500);
    let report = try_sync(&mut b);
    sharedfolder::set_write_limit_for_test(sharedfolder::MAX_FILE_BYTES);
    report.expect("B receives");
    assert_eq!(b.load_document(document).unwrap().note, "Nota de A");
    b.save_note(document, "Outra nota de B").unwrap();
    limit(1500);
    let report = try_sync(&mut b);
    sharedfolder::set_write_limit_for_test(sharedfolder::MAX_FILE_BYTES);
    waits(&report.expect("B sends in format 8"));
    still_eight(&b);
    // And A receives what B sent.
    limit(16);
    let report = try_sync(&mut a);
    sharedfolder::set_write_limit_for_test(sharedfolder::MAX_FILE_BYTES);
    report.expect("A receives");
    assert_eq!(a.load_document(document).unwrap().note, "Outra nota de B");
    still_eight(&a);
    // With room, the folder moves to format 9.
    let mut promoted = false;
    for _ in 0..2 {
        promoted |= sync(&mut a).promoted;
    }
    assert!(promoted);
}

/// Revisão do pacote, achado 5: the promotion waits while the folder has a
/// copy the cloud made of the file, by the same rule as the folder card of
/// 0.51 (here the copy OneDrive names after the computer).
#[test]
fn the_promotion_waits_for_a_conflict_copy_the_folder_card_warns_about() {
    let AtEight { drive, mut a, .. } = at_eight();
    let copy = drive.with_file_name("ponte-sync-NOTEBOOK-01.ponte");
    std::fs::copy(&drive, &copy).unwrap();
    let scan = crate::folder_health::scan_all(
        &[drive.display().to_string()],
        std::time::SystemTime::now(),
        &crate::folder_health::HashMemo::default(),
    )
    .remove(0);
    assert!(
        crate::folder_health::conflict_warning(&scan)
            .unwrap_or_default()
            .contains("ponte-sync-NOTEBOOK-01.ponte"),
        "the card warns about it: {scan:?}"
    );
    on();
    let report = sync(&mut a);
    assert!(!report.promoted, "{report:?}");
    let wait = report.promotion_wait.unwrap_or_default();
    assert!(wait.contains("ponte-sync-NOTEBOOK-01.ponte"), "{wait}");
    assert!(!crate::sharedfolder::nine_path(&drive).exists());
    std::fs::remove_file(&copy).unwrap();
    assert!(sync(&mut a).promoted);
}

/// Revisão do pacote, achado 7: the refusal by size while a computer takes
/// its first file of format 9 reads as one sentence after another, with no
/// doubled period.
#[test]
fn the_size_refusal_while_completing_the_update_reads_cleanly() {
    let AtEight {
        mut a,
        mut b,
        document,
        ..
    } = at_eight();
    on();
    assert!(sync(&mut a).promoted);
    let owner = crate::attachments::Owner::Document(document);
    b.add_attachment(owner, "grande.txt", "", &vec![b'x'; 6000])
        .unwrap();
    crate::sharedfolder::set_write_limit_for_test(2000);
    let error = try_sync(&mut b).map(|_| ()).unwrap_err().to_string();
    crate::sharedfolder::set_write_limit_for_test(crate::sharedfolder::MAX_FILE_BYTES);
    assert!(
        error.starts_with(
            "A atualização do arquivo da pasta parou: O acervo passa de 512 MB e não cabe no arquivo de sincronização"
        ),
        "{error}"
    );
    assert!(!error.contains(".."), "{error}");
    assert!(
        error.contains(&format!(
            "Nada foi gravado. {} Os arquivos antigos ficaram como estavam",
            crate::folder_health::ADVICE
        )),
        "{error}"
    );
    assert!(
        error.ends_with("a próxima sincronização continua de onde parou."),
        "{error}"
    );
}
