//! End-to-end native messaging against real SQLite and per-user Windows DPAPI.
//! Fixtures are synthetic; no browser, network, or real SIGADOC document is used.
#![cfg(windows)]

use ponteproj::{
    model::{Capture, Coverage},
    native,
    storage::Store,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{self, Cursor, Write};
use std::path::Path;

const PRIVATE_MARKER: &str = "CONTEUDO_SINTETICO_RESERVADO_6B24AD0E";

fn fixture() -> Capture {
    Capture {
        schema_version: 1,
        request_id: uuid::Uuid::new_v4().to_string(),
        source_url:
            "https://www.sigadoc.mt.gov.br/sigaex/app/expediente/doc?sigla=TESTE-PRO-2026%2F12345"
                .into(),
        document_number: "TESTE-PRO-2026/12345".into(),
        title: "Ofício sintético de validação do transporte".into(),
        captured_at: "2026-11-01T12:00:00Z".into(),
        text: format!("{PRIVATE_MARKER}: solicitação fictícia de material de escritório."),
        metadata: BTreeMap::from([("Unidade".into(), "Unidade fictícia de testes".into())]),
        references: vec![],
        coverage: Coverage {
            adapter: "integration-synthetic-v1".into(),
            recognized: vec!["identidade".into(), "texto".into()],
            pending: vec!["Anexo sintético ainda não capturado".into()],
        },
    }
}

fn request(capture: &Capture) -> Value {
    json!({"protocol_version": 1, "action": "import_capture", "capture": capture})
}

// Construct the wire independently of native::write_frame, including its byte length.
fn append_frame(stream: &mut Vec<u8>, body: &[u8]) {
    assert!(!body.is_empty() && body.len() <= native::MAX_FRAME);
    stream.extend_from_slice(&(body.len() as u32).to_le_bytes());
    stream.extend_from_slice(body);
}

fn append_json(stream: &mut Vec<u8>, value: &Value) {
    append_frame(stream, &serde_json::to_vec(value).unwrap());
}

fn responses(mut bytes: &[u8]) -> Vec<Value> {
    let mut result = Vec::new();
    while !bytes.is_empty() {
        assert!(bytes.len() >= 4, "partial response frame header");
        let length = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
        assert!(length > 0 && length <= native::MAX_FRAME);
        assert!(bytes.len() >= 4 + length, "partial response frame body");
        result.push(serde_json::from_slice(&bytes[4..4 + length]).unwrap());
        bytes = &bytes[4 + length..];
    }
    result
}

fn count_rows(path: &Path, table: &str) -> i64 {
    assert!(["documents", "captures", "requests"].contains(&table));
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
        row.get(0)
    })
    .unwrap()
}

#[test]
fn valid_framed_import_and_replay_commit_before_every_ui_callback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("native.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let capture = fixture();
    let value = request(&capture);
    let mut input = Vec::new();
    append_json(&mut input, &value);
    append_json(&mut input, &value);
    let mut output = Vec::new();
    let mut callbacks = Vec::new();

    native::serve(&mut Cursor::new(input), &mut output, &mut store, |id| {
        // A separate connection must already see a complete, decryptable record.
        let observer = Store::open(&path).unwrap();
        let detail = observer.load_document(id).unwrap();
        assert_eq!(detail.capture, capture);
        assert_eq!(observer.list_documents("").unwrap().len(), 1);
        callbacks.push((id, detail.summary.latest_capture_id));
        true
    })
    .unwrap();

    let replies = responses(&output);
    assert_eq!(replies.len(), 2);
    assert_eq!(callbacks.len(), 2);
    assert_eq!(callbacks[0], callbacks[1]);
    for reply in &replies {
        assert_eq!(reply["protocol_version"], 1);
        assert_eq!(reply["ok"], true);
        assert_eq!(reply["request_id"], capture.request_id);
        assert_eq!(reply["ui_opened"], true);
        assert_eq!(reply["document_id"].as_i64().unwrap(), callbacks[0].0);
        assert_eq!(reply["capture_id"].as_i64().unwrap(), callbacks[0].1);
    }
    assert_eq!(replies[0]["duplicate"], false);
    assert_eq!(replies[0]["outcome"], "imported");
    assert_eq!(replies[1]["duplicate"], true);
    assert_eq!(replies[1]["outcome"], "confirmed");
    assert_eq!(count_rows(&path, "documents"), 1);
    assert_eq!(count_rows(&path, "captures"), 1);
    assert_eq!(count_rows(&path, "requests"), 1);

    // SQLite's real on-disk snapshot must not expose documentary content.
    let backup_path = dir.path().join("encrypted-backup.sqlite3");
    store.backup(&backup_path).unwrap();
    let backup_bytes = std::fs::read(&backup_path).unwrap();
    for secret in [
        PRIVATE_MARKER,
        capture.title.as_str(),
        capture.document_number.as_str(),
    ] {
        assert!(!backup_bytes
            .windows(secret.len())
            .any(|window| window == secret.as_bytes()));
    }
}

struct DisconnectedBrowser;

impl Write for DisconnectedBrowser {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "synthetic browser disconnected",
        ))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn committed_import_survives_lost_ack_and_retry_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lost-ack.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let capture = fixture();
    let mut input = Vec::new();
    append_json(&mut input, &request(&capture));
    let mut first_callback = None;
    let failure = native::serve(
        &mut Cursor::new(input.clone()),
        &mut DisconnectedBrowser,
        &mut store,
        |id| {
            first_callback = Some(id);
            false
        },
    );
    assert!(failure.is_err());
    let id = first_callback
        .expect("UI callback should run after commit even when the ACK cannot be delivered");
    let persisted_capture_id = Store::open(&path)
        .unwrap()
        .load_document(id)
        .unwrap()
        .summary
        .latest_capture_id;
    // Close/reopen also proves persistence across a native-host restart.
    drop(store);
    let mut store = Store::open(&path).unwrap();
    let mut output = Vec::new();
    native::serve(
        &mut Cursor::new(input),
        &mut output,
        &mut store,
        |retry_id| {
            assert_eq!(retry_id, id);
            false
        },
    )
    .unwrap();
    let replies = responses(&output);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0]["ok"], true);
    assert_eq!(replies[0]["duplicate"], true);
    assert_eq!(replies[0]["outcome"], "confirmed");
    assert_eq!(replies[0]["document_id"].as_i64(), Some(id));
    assert_eq!(
        replies[0]["capture_id"].as_i64(),
        Some(persisted_capture_id)
    );
    assert_eq!(replies[0]["ui_opened"], false);
    assert_eq!(count_rows(&path, "documents"), 1);
    assert_eq!(count_rows(&path, "captures"), 1);
    assert_eq!(count_rows(&path, "requests"), 1);
}

#[test]
fn unchanged_import_warns_without_opening_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unchanged.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let first = fixture();
    let mut same = first.clone();
    same.request_id = uuid::Uuid::new_v4().to_string();
    same.captured_at = "2026-11-03T09:30:00Z".into();
    let mut input = Vec::new();
    append_json(&mut input, &request(&first));
    append_json(&mut input, &request(&same));
    let mut output = Vec::new();
    let mut opened = Vec::new();
    native::serve(&mut Cursor::new(input), &mut output, &mut store, |id| {
        opened.push(id);
        true
    })
    .unwrap();
    let replies = responses(&output);
    assert_eq!(replies.len(), 2);
    assert_eq!(opened.len(), 1, "an ignored import never opens the Ponte");
    assert_eq!(replies[0]["outcome"], "imported");
    assert_eq!(replies[0]["ui_opened"], true);
    let ignored = &replies[1];
    assert_eq!(ignored["ok"], true);
    assert_eq!(ignored["request_id"], same.request_id);
    assert_eq!(ignored["duplicate"], true);
    assert_eq!(ignored["outcome"], "unchanged");
    assert_eq!(ignored["ui_opened"], false);
    assert_eq!(ignored["capture_id"], replies[0]["capture_id"]);
    let message = ignored["message"].as_str().unwrap();
    assert!(message.starts_with("Importação ignorada"), "{message}");
    assert!(message.contains("Nada foi gravado"), "{message}");
    let stored_at: String =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row("SELECT created_at FROM captures", [], |row| row.get(0))
            .unwrap();
    assert!(
        message.contains(&ponteproj::labels::datetime(&stored_at)),
        "{message}"
    );
    assert_eq!(count_rows(&path, "documents"), 1);
    assert_eq!(count_rows(&path, "captures"), 1);
    assert_eq!(count_rows(&path, "requests"), 1);
}

#[test]
fn incompatible_versions_invalid_origins_and_bad_json_never_persist_or_echo_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rejected.sqlite3");
    let mut store = Store::open(&path).unwrap();
    let capture = fixture();
    let mut input = Vec::new();
    let mut incompatible = request(&capture);
    incompatible["protocol_version"] = json!(99);
    append_json(&mut input, &incompatible);
    let mut future_capture = capture.clone();
    future_capture.schema_version = 99;
    append_json(&mut input, &request(&future_capture));
    let mut bad_origin = capture.clone();
    bad_origin.source_url = format!("https://www.sigadoc.mt.gov.br.evil.example/{PRIVATE_MARKER}");
    append_json(&mut input, &request(&bad_origin));
    let mut bad_uuid = capture.clone();
    bad_uuid.request_id = PRIVATE_MARKER.into();
    append_json(&mut input, &request(&bad_uuid));
    append_frame(&mut input, format!("{{\"protocol_version\":1,\"action\":\"import_capture\",\"capture\":{{\"secret\":\"{PRIVATE_MARKER}\"").as_bytes());
    let mut malformed_shape = request(&capture);
    malformed_shape[PRIVATE_MARKER] = json!(PRIVATE_MARKER);
    append_json(&mut input, &malformed_shape);
    let mut output = Vec::new();
    let mut callback_count = 0;
    native::serve(&mut Cursor::new(input), &mut output, &mut store, |_| {
        callback_count += 1;
        false
    })
    .unwrap();
    let replies = responses(&output);
    assert_eq!(replies.len(), 6);
    assert_eq!(callback_count, 0);
    assert_eq!(replies[0]["error"]["code"], "incompatible_version");
    assert_eq!(replies[1]["error"]["code"], "import_failed");
    assert_eq!(replies[2]["error"]["code"], "import_failed");
    assert_eq!(replies[3]["error"]["code"], "import_failed");
    assert_eq!(replies[4]["error"]["code"], "invalid_request");
    assert_eq!(replies[5]["error"]["code"], "invalid_request");
    for reply in replies {
        assert_eq!(reply["ok"], false);
        assert!(reply["error"]["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty()));
        assert!(reply.get("document_id").is_none());
        assert!(reply.get("ui_opened").is_none());
    }
    let visible_errors = String::from_utf8_lossy(&output);
    for secret in [
        PRIVATE_MARKER,
        capture.title.as_str(),
        capture.document_number.as_str(),
        capture.text.as_str(),
    ] {
        assert!(
            !visible_errors.contains(secret),
            "native errors leaked synthetic documentary content"
        );
    }
    assert!(store.list_documents("").unwrap().is_empty());
    assert_eq!(count_rows(&path, "documents"), 0);
    assert_eq!(count_rows(&path, "captures"), 0);
    assert_eq!(count_rows(&path, "requests"), 0);
}
