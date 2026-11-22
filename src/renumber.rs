//! Manual records change number when a creation made offline (or in a
//! shared folder) is replayed onto the shared copy: the number was "the
//! highest of this library plus one", and the shared copy may already have a
//! record with it. This module tells the person (a local notice, never
//! synchronized), keeps the old number findable on this computer and says
//! which numbers are still provisional. It changes nothing about how numbers
//! are given.
//!
//! The identity of a record across the change is its creation request id:
//! every snapshot row of `requests` carries it, so the same record is found
//! on both sides of an adoption without any new data in the shared copy. A
//! record with no request on either side has no identity and gives no notice
//! (the title is never used to guess).
use crate::model::{Capture, MANUAL_PENDING};
use crate::protection::{seal, unseal};
use crate::storage::Store;
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// How many notices (dismissed ones included, they keep the old number
/// findable) this library remembers.
const KEEP_NOTICES: i64 = 300;

/// "Record X went from number A to number B when this computer adopted the
/// shared copy." Local to this computer: it is not part of the snapshot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenumberNotice {
    /// Row of the local table (not stored in the payload).
    #[serde(skip)]
    pub id: i64,
    /// Identity of the record: the request id of its creation.
    pub uid: String,
    pub title: String,
    pub from: String,
    pub to: String,
    /// When this computer saw the change (RFC 3339).
    pub at: String,
    #[serde(default)]
    pub dismissed: bool,
}

impl RenumberNotice {
    /// The sentence shown to the person.
    pub fn sentence(&self) -> String {
        format!(
            "O registro “{}” passou de {} para {} ao sincronizar.",
            short_title(&self.title),
            self.from,
            self.to
        )
    }
}

fn short_title(title: &str) -> String {
    let title = title.trim();
    if title.chars().count() > 70 {
        format!("{}…", title.chars().take(70).collect::<String>().trim_end())
    } else {
        title.to_owned()
    }
}

/// The one sentence for several notices (the sync summary, the card in Hoje).
pub fn summarize(notices: &[RenumberNotice]) -> Option<String> {
    match notices {
        [] => None,
        [one] => Some(one.sentence()),
        many => Some(format!(
            "{} registros mudaram de número ao sincronizar.",
            many.len()
        )),
    }
}

fn same_number(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.split_whitespace().collect::<String>().to_uppercase();
    norm(a) == norm(b)
}

fn read_capture(blob: &[u8]) -> Option<Capture> {
    serde_json::from_slice(&unseal(blob).ok()?).ok()
}

/// The manual numbers this library presents, by record identity, read before
/// a snapshot replaces the library.
#[derive(Default)]
pub(crate) struct Presented(BTreeMap<String, (String, String)>);

impl Presented {
    /// Reads them (best effort: a row that cannot be read gives no notice).
    pub(crate) fn read(conn: &Connection) -> Self {
        let mut out = BTreeMap::new();
        let _ = (|| -> Result<()> {
            let mut statement = conn.prepare(
                "SELECT c.payload,
                        (SELECT r.request_id FROM requests r JOIN captures rc ON rc.id=r.capture_id
                          WHERE r.document_id=d.id ORDER BY rc.id LIMIT 1)
                   FROM documents d JOIN captures c ON c.id=d.latest_capture_id",
            )?;
            let rows = statement.query_map([], |r| {
                Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            for row in rows {
                let (blob, uid) = row?;
                let Some(uid) = uid else { continue };
                let Some(capture) = read_capture(&blob) else {
                    continue;
                };
                if capture.is_manual() && capture.document_number != MANUAL_PENDING {
                    out.insert(uid, (capture.document_number, capture.title));
                }
            }
            Ok(())
        })();
        Self(out)
    }

    /// Compares with the library as it is now (the snapshot adopted, inside
    /// the same transaction) and records one notice for each record whose
    /// number differs. Best effort: the exchange never fails because of it.
    /// `event` names the adopted state (revision and generation), so the same
    /// transition of the same state is never recorded twice.
    pub(crate) fn record_changes(&self, conn: &Connection, event: &str) {
        let _ = (|| -> Result<()> {
            for (uid, (number, title)) in &self.0 {
                let after: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT c.payload FROM requests r JOIN documents d ON d.id=r.document_id
                           JOIN captures c ON c.id=d.latest_capture_id WHERE r.request_id=?1",
                        [uid],
                        |r| r.get(0),
                    )
                    .optional()?;
                let Some(after) = after.as_deref().and_then(read_capture) else {
                    continue;
                };
                if !after.is_manual()
                    || after.document_number == MANUAL_PENDING
                    || same_number(&after.document_number, number)
                {
                    continue;
                }
                let notice = RenumberNotice {
                    id: 0,
                    uid: uid.clone(),
                    title: title.clone(),
                    from: number.clone(),
                    to: after.document_number,
                    at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    dismissed: false,
                };
                let dedup = Sha256::digest(
                    format!("{uid}\n{}\n{}\n{event}", notice.from, notice.to).as_bytes(),
                )
                .to_vec();
                conn.execute(
                    "INSERT OR IGNORE INTO renumber_notices(dedup,payload) VALUES(?1,?2)",
                    params![dedup, seal(&serde_json::to_vec(&notice)?)?],
                )?;
            }
            conn.execute(
                "DELETE FROM renumber_notices WHERE id NOT IN
                   (SELECT id FROM renumber_notices ORDER BY id DESC LIMIT ?1)",
                [KEEP_NOTICES],
            )?;
            Ok(())
        })();
    }
}

impl Store {
    /// The local table of notices. Idempotent, like the other local tables;
    /// nothing in it is part of a snapshot, and adopting one leaves it alone.
    pub(crate) fn ensure_renumber_notices(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS renumber_notices(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                dedup BLOB NOT NULL UNIQUE,
                payload BLOB NOT NULL
            );",
        )?;
        Ok(())
    }

    fn all_renumber_notices(&self) -> Result<Vec<RenumberNotice>> {
        self.cached("renumber_notices", || {
            let mut statement = self
                .conn
                .prepare("SELECT id,payload FROM renumber_notices ORDER BY id DESC")?;
            let rows = statement.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (id, blob) = row?;
                // A notice that cannot be read (another account's key) is
                // only skipped.
                let Ok(plain) = unseal(&blob) else { continue };
                if let Ok(mut notice) = serde_json::from_slice::<RenumberNotice>(&plain) {
                    notice.id = id;
                    out.push(notice);
                }
            }
            Ok(out)
        })
    }

    /// The notices the person has not dismissed, newest first.
    pub fn pending_renumber_notices(&self) -> Result<Vec<RenumberNotice>> {
        Ok(self
            .all_renumber_notices()?
            .into_iter()
            .filter(|n| !n.dismissed)
            .collect())
    }

    /// Notices recorded after row `last` (what one exchange just found),
    /// oldest first.
    pub(crate) fn renumber_notices_after(&self, last: i64) -> Result<Vec<RenumberNotice>> {
        Ok(self
            .all_renumber_notices()?
            .into_iter()
            .filter(|n| n.id > last)
            .rev()
            .collect())
    }

    /// The last row of the table (0: empty).
    pub(crate) fn last_renumber_notice_id(&self) -> i64 {
        self.conn
            .query_row("SELECT coalesce(max(id),0) FROM renumber_notices", [], |r| {
                r.get(0)
            })
            .unwrap_or(0)
    }

    /// Marks a notice as read. It stays as history, so the old number keeps
    /// being found on this computer.
    pub fn dismiss_renumber_notice(&self, id: i64) -> Result<()> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT payload FROM renumber_notices WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(blob) = blob else { return Ok(()) };
        let mut notice: RenumberNotice = serde_json::from_slice(&unseal(&blob)?)?;
        if notice.dismissed {
            return Ok(());
        }
        notice.dismissed = true;
        self.conn.execute(
            "UPDATE renumber_notices SET payload=?1 WHERE id=?2",
            params![seal(&serde_json::to_vec(&notice)?)?, id],
        )?;
        Ok(())
    }

    /// Dismisses every notice.
    pub fn dismiss_all_renumber_notices(&self) -> Result<()> {
        for notice in self.pending_renumber_notices()? {
            self.dismiss_renumber_notice(notice.id)?;
        }
        Ok(())
    }

    /// The numbers records had before this computer adopted a copy that
    /// renumbered them, by the document they are now (newest change first).
    pub(crate) fn old_numbers_by_document(&self) -> Result<BTreeMap<i64, Vec<String>>> {
        self.cached("renumber_old_numbers", || {
            let mut out: BTreeMap<i64, Vec<String>> = BTreeMap::new();
            for notice in self.all_renumber_notices()? {
                let document: Option<i64> = self
                    .conn
                    .query_row(
                        "SELECT document_id FROM requests WHERE request_id=?1",
                        [&notice.uid],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(document) = document {
                    let numbers = out.entry(document).or_default();
                    if !numbers.contains(&notice.from) {
                        numbers.push(notice.from);
                    }
                }
            }
            Ok(out)
        })
    }

    /// "Número anterior neste computador": the numbers this record had here
    /// before, except the one it has now.
    pub fn previous_numbers(&self, document_id: i64) -> Vec<String> {
        let now = self.document_number_by_id(document_id).ok().flatten();
        self.old_numbers_by_document()
            .ok()
            .and_then(|map| map.get(&document_id).cloned())
            .unwrap_or_default()
            .into_iter()
            .filter(|number| now.as_deref().is_none_or(|now| !same_number(now, number)))
            .collect()
    }

    /// Documents whose creation is still waiting in the outgoing queue: a
    /// manual record there shows a number this library gave it, and the shared
    /// copy may give it another one when the creation is replayed there.
    pub(crate) fn provisional_documents(&self) -> Result<BTreeSet<i64>> {
        self.cached("provisional_documents", || {
            let waiting: i64 = self
                .conn
                .query_row("SELECT count(*) FROM offline_outbox", [], |r| r.get(0))?;
            let mut out = BTreeSet::new();
            if waiting == 0 {
                return Ok(out);
            }
            for entry in self.list_offline_outbox()? {
                if let crate::intranet::IntranetMutation::ImportCapture { capture } =
                    &entry.mutation
                {
                    if capture.is_manual() && capture.document_number == MANUAL_PENDING {
                        out.extend(
                            entry
                                .created
                                .iter()
                                .filter(|c| c.kind == crate::idmap::IdKind::Document)
                                .map(|c| c.id),
                        );
                    }
                }
            }
            Ok(out)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::intranet::{IntranetConfig, IntranetMode};
    use crate::manual::ManualFields;
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

    fn fields(title: &str, text: &str) -> ManualFields {
        ManualFields {
            kind: "ligacao".into(),
            title: title.into(),
            origin: "Gabinete".into(),
            document_date: String::new(),
            text: text.into(),
        }
    }

    /// Two computers sharing one folder, both already in sync on a library
    /// with one SIGADOC document (the copy exists and each one has adopted it).
    struct Pair {
        _dir: tempfile::TempDir,
        a: Store,
        b: Store,
    }

    fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
        let cfg = store.load_intranet_config().unwrap();
        store
            .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
            .unwrap()
    }

    fn pair() -> Pair {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let files = vec![drive.display().to_string()];
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", files.clone()))
            .unwrap();
        a.import_capture(&crate::model::sample_capture()).unwrap();
        sync(&mut a);
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", files))
            .unwrap();
        sync(&mut b);
        Pair { _dir: dir, a, b }
    }

    fn create(store: &mut Store, title: &str, text: &str) -> (i64, String) {
        let receipt = store.create_manual_document(&fields(title, text)).unwrap();
        let number = store
            .load_document(receipt.document_id)
            .unwrap()
            .summary
            .document_number;
        (receipt.document_id, number)
    }

    fn number_of(store: &Store, title: &str) -> Option<String> {
        store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.title == title)
            .map(|d| d.document_number)
    }

    fn id_of(store: &Store, title: &str) -> i64 {
        store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.title == title)
            .unwrap_or_else(|| panic!("sem “{title}”"))
            .id
    }

    fn text_of(store: &Store, number: &str) -> String {
        let id = store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.document_number == number)
            .unwrap_or_else(|| panic!("sem {number}"))
            .id;
        store.load_document(id).unwrap().capture.text
    }

    /// The collision of the observed incident: A publishes its record, B
    /// (still without that copy) created one with the same number.
    fn collide(p: &mut Pair) -> (String, String) {
        let (_, x) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        let (_, y) = create(&mut p.b, "Registro de B", "Texto de B");
        assert_eq!(x, y, "the two libraries numbered alike");
        (x, y)
    }

    /// The defect: B creates a manual record offline and edits it before the
    /// first exchange; the shared copy already has another record with the
    /// same number. B's creation is numbered again on replay, and its edit,
    /// addressed by the old number, must follow it, never land on the other
    /// record.
    #[test]
    fn an_edit_of_a_renumbered_creation_never_lands_on_the_record_that_took_its_number() {
        let mut p = pair();
        let (x_number, _) = collide(&mut p);
        let y = id_of(&p.b, "Registro de B");
        p.b.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        sync(&mut p.b);
        sync(&mut p.a);

        for store in [&p.a, &p.b] {
            assert_eq!(
                number_of(store, "Registro de A").as_deref(),
                Some(x_number.as_str()),
                "A's record keeps its number and its title (B's edit replaced it?)"
            );
            assert_eq!(
                text_of(store, &x_number),
                "Texto de A",
                "B's edit was applied to A's record"
            );
            let b_number = number_of(store, "Registro de B").expect("B's record exists");
            assert_ne!(b_number, x_number);
            assert_eq!(text_of(store, &b_number), "Texto de B, editado");
        }
    }

    /// The same defect on the other road: the edit reaches a third computer
    /// through the journal of another folder copy, not through its own queue.
    #[test]
    fn an_edit_that_arrives_in_the_journal_of_another_copy_follows_its_creation_too() {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        let pen = dir.path().join("Pendrive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        std::fs::create_dir_all(pen.parent().unwrap()).unwrap();
        let both = vec![drive.display().to_string(), pen.display().to_string()];
        let only = |path: &std::path::Path| vec![path.display().to_string()];
        let sync_with = |store: &mut Store, files: Vec<String>| {
            let mut cfg = store.load_intranet_config().unwrap();
            cfg.folder_files = files;
            store.save_intranet_config(&cfg).unwrap();
            store
                .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
                .unwrap()
        };
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", both.clone()))
            .unwrap();
        a.import_capture(&crate::model::sample_capture()).unwrap();
        sync_with(&mut a, both.clone());
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", both.clone()))
            .unwrap();
        sync_with(&mut b, both.clone());

        // A writes only to the drive, with more changes than B will make
        // (so its copy is the base); B only to the pen drive.
        let (x, _) = create(&mut a, "Registro de A", "Texto de A");
        for n in 0..4 {
            let mut task = crate::tasks::Task::new(x);
            task.title = format!("Tarefa {n}");
            a.save_task(&task).unwrap();
        }
        sync_with(&mut a, only(&drive));
        let (y, y_number) = create(&mut b, "Registro de B", "Texto de B");
        b.update_manual_document(y, &fields("Registro de B", "Texto de B, editado"))
            .unwrap();
        sync_with(&mut b, only(&pen));
        let revision = |path: &std::path::Path| {
            crate::sharedfolder::read_header(path)
                .unwrap()
                .unwrap()
                .revision
        };
        assert!(revision(&drive) > revision(&pen), "the drive is the base");

        // A reads both: B's creation and edit arrive from the other copy.
        let report = sync_with(&mut a, both.clone());
        assert!(report.merged_foreign >= 2, "{}", report.summary());
        assert_eq!(report.conflicts, 0, "{}", report.summary());
        let x_number = number_of(&a, "Registro de A").unwrap();
        assert_eq!(x_number, y_number, "A kept its number");
        assert_eq!(text_of(&a, &x_number), "Texto de A");
        let b_number = number_of(&a, "Registro de B").unwrap();
        assert_ne!(b_number, x_number);
        assert_eq!(text_of(&a, &b_number), "Texto de B, editado");
        // And B, adopting that copy, is told.
        let report = sync_with(&mut b, both);
        assert_eq!(report.renumbered.len(), 1, "{}", report.summary());
        assert_eq!(number_of(&b, "Registro de B").unwrap(), b_number);
        assert_eq!(text_of(&b, &x_number), "Texto de A");
    }

    #[test]
    fn an_edit_whose_creation_was_refused_becomes_a_conflict_and_a_moved_one_is_rewritten() {
        use crate::idmap::{CreatedId, IdKind, IdMap};
        use crate::intranet::{retarget_manual_edit, IntranetMutation};
        let dir = tempfile::tempdir().unwrap();
        let mut copy = Store::open(&dir.path().join("copy.sqlite3")).unwrap();
        let (there, number) = create(&mut copy, "Registro na cópia", "Texto");
        let edit = |number: &str| {
            let mut capture = fields("Registro de B", "Editado").capture(number, "Teste");
            capture.request_id = uuid::Uuid::new_v4().to_string();
            IntranetMutation::ImportCapture { capture }
        };
        let local = CreatedId::new(IdKind::Document, "", 7);
        let mut ids = IdMap::default();
        ids.learn(
            &[local.clone()],
            Some(&[CreatedId::new(IdKind::Document, "", there)]),
        );
        // The creation went to a record that is numbered differently there.
        let moved = retarget_manual_edit(edit("MANUAL-2026/0099"), Some(7), &ids, &copy).unwrap();
        let IntranetMutation::ImportCapture { capture } = moved else {
            panic!()
        };
        assert_eq!(capture.document_number, number);
        assert_eq!(capture.source_url, format!("ponte://manual/{number}"));
        // Same number there: left as it was.
        let same = retarget_manual_edit(edit(&number), Some(7), &ids, &copy).unwrap();
        assert!(
            matches!(same, IntranetMutation::ImportCapture { capture } if capture.document_number == number)
        );
        // The creation was refused: the edit is refused with it.
        let mut refused = IdMap::default();
        refused.learn(&[local], None);
        assert!(retarget_manual_edit(edit("MANUAL-2026/0099"), Some(7), &refused, &copy).is_err());
        // Nothing known of the document (shared history): untouched, as is a
        // SIGADOC capture.
        let untouched =
            retarget_manual_edit(edit("MANUAL-2026/0099"), Some(8), &ids, &copy).unwrap();
        assert!(
            matches!(untouched, IntranetMutation::ImportCapture { capture } if capture.document_number == "MANUAL-2026/0099")
        );
        let official = IntranetMutation::ImportCapture {
            capture: crate::model::sample_capture(),
        };
        let official = retarget_manual_edit(official, Some(7), &ids, &copy).unwrap();
        assert!(
            matches!(official, IntranetMutation::ImportCapture { capture } if !capture.is_manual())
        );
    }

    #[test]
    fn the_renumber_notice_appears_once_after_a_collision_and_not_without_one() {
        let mut p = pair();
        // No collision: B's record is the only one, it keeps its number.
        let (_, z_number) = create(&mut p.b, "Registro sem colisão", "Texto");
        let report = sync(&mut p.b);
        assert!(report.renumbered.is_empty(), "{}", report.summary());
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
        assert_eq!(number_of(&p.b, "Registro sem colisão").unwrap(), z_number);
        sync(&mut p.a);

        // Collision: A publishes a record; B's, made without that copy,
        // takes the next number when it is sent.
        let (_, x_number) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        let (_, y_number) = create(&mut p.b, "Registro de B", "Texto de B");
        assert_ne!(x_number, z_number);
        let report = sync(&mut p.b);
        let new_number = number_of(&p.b, "Registro de B").unwrap();
        if y_number == new_number {
            panic!("the record kept its number: {y_number}");
        }
        assert_eq!(report.renumbered.len(), 1);
        assert!(
            report.summary().contains(&format!(
                "O registro “Registro de B” passou de {y_number} para {new_number} ao sincronizar"
            )),
            "{}",
            report.summary()
        );
        let notices = p.b.pending_renumber_notices().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].title, "Registro de B");
        assert_eq!((&notices[0].from, &notices[0].to), (&y_number, &new_number));
        assert_eq!(
            notices[0].sentence(),
            format!(
                "O registro “Registro de B” passou de {y_number} para {new_number} ao sincronizar."
            )
        );
        // Exactly once: the next exchanges, and the one on A, add nothing.
        sync(&mut p.b);
        sync(&mut p.a);
        sync(&mut p.b);
        assert_eq!(p.b.pending_renumber_notices().unwrap().len(), 1);
        assert!(p.a.pending_renumber_notices().unwrap().is_empty());
        // The notice is local: the shared snapshot carries nothing about it.
        let snapshot = serde_json::to_string(&p.b.export_snapshot().unwrap()).unwrap();
        assert!(!snapshot.contains("renumber"));
    }

    #[test]
    fn a_record_without_identity_gives_no_notice() {
        let mut p = pair();
        let (_, x) = create(&mut p.a, "Registro de A", "Texto de A");
        sync(&mut p.a);
        let (y_id, y) = create(&mut p.b, "Registro de B", "Texto de B");
        assert_eq!(x, y);
        // A record that, on this computer, knows no request of its creation
        // (the library predates them): it is renumbered like the others, but
        // nothing says it is the same record, and the title is not a proof.
        p.b.conn
            .execute("DELETE FROM requests WHERE document_id=?1", [y_id])
            .unwrap();
        let report = sync(&mut p.b);
        assert_ne!(number_of(&p.b, "Registro de B").unwrap(), y);
        assert!(report.renumbered.is_empty(), "{}", report.summary());
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
    }

    #[test]
    fn dismissing_a_notice_hides_it_and_applying_the_same_snapshot_again_does_not_bring_it_back() {
        let mut p = pair();
        collide(&mut p);
        sync(&mut p.b);
        let notice = p.b.pending_renumber_notices().unwrap().remove(0);
        p.b.dismiss_renumber_notice(notice.id).unwrap();
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
        // The same snapshot, adopted again.
        let snapshot = p.b.export_snapshot().unwrap();
        for _ in 0..2 {
            p.b.apply_snapshot_and(
                &snapshot,
                |_, _| unreachable!(),
                |_| unreachable!(),
                |_| Ok(()),
            )
            .unwrap();
        }
        sync(&mut p.b);
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
        // Dismissing twice, or an unknown one, is harmless.
        p.b.dismiss_renumber_notice(notice.id).unwrap();
        p.b.dismiss_renumber_notice(9999).unwrap();
        // And a notice that is not dismissed survives the same adoption,
        // still one.
        let mut q = pair();
        collide(&mut q);
        sync(&mut q.b);
        let snapshot = q.b.export_snapshot().unwrap();
        q.b.apply_snapshot_and(
            &snapshot,
            |_, _| unreachable!(),
            |_| unreachable!(),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(q.b.pending_renumber_notices().unwrap().len(), 1);
    }

    #[test]
    fn a_scratch_library_never_writes_a_notice() {
        let moved_copy = |store: &Store, title: &str, to: &str| {
            let mut copy = store.export_snapshot().unwrap();
            for row in &mut copy.captures {
                if row.capture.title == title {
                    row.capture.document_number = to.to_owned();
                    row.capture.source_url = format!("ponte://manual/{to}");
                }
            }
            copy
        };
        let mut p = pair();
        let (_, y) = create(&mut p.b, "Registro de B", "Texto de B");
        let moved = format!("MANUAL-{}/0042", &y[7..11]);
        let other = moved_copy(&p.b, "Registro de B", &moved);
        // The scratch road (the temporary library of an exchange): nothing.
        p.b.apply_snapshot(&other, |_, _| unreachable!(), |_| unreachable!())
            .unwrap();
        assert!(p.b.pending_renumber_notices().unwrap().is_empty());
        assert_eq!(number_of(&p.b, "Registro de B").unwrap(), moved);
        // The library the person works in: the same change is told.
        let mut q = pair();
        let (_, y) = create(&mut q.b, "Registro de B", "Texto de B");
        let other = moved_copy(&q.b, "Registro de B", &moved);
        q.b.apply_snapshot_and(
            &other,
            |_, _| unreachable!(),
            |_| unreachable!(),
            |_| Ok(()),
        )
        .unwrap();
        let notices = q.b.pending_renumber_notices().unwrap();
        assert_eq!(notices.len(), 1);
        assert_eq!(
            (notices[0].from.as_str(), notices[0].to.as_str()),
            (y.as_str(), moved.as_str())
        );
    }

    #[test]
    fn the_old_number_is_found_by_search_and_shown_in_the_sheet_while_the_history_exists() {
        let mut p = pair();
        let (x, _) = collide(&mut p);
        sync(&mut p.b);
        let y = id_of(&p.b, "Registro de B");
        let hits: Vec<i64> = p
            .b
            .list_documents(&x)
            .unwrap()
            .iter()
            .map(|d| d.id)
            .collect();
        assert!(
            hits.contains(&y),
            "the record renumbered is found by its old number"
        );
        assert!(
            hits.contains(&id_of(&p.b, "Registro de A")),
            "and the one that holds it now"
        );
        assert_eq!(p.b.previous_numbers(y), vec![x.clone()]);
        assert!(p
            .b
            .previous_numbers(id_of(&p.b, "Registro de A"))
            .is_empty());
        // Dismissed, it stays findable (the history is what finds it).
        p.b.dismiss_all_renumber_notices().unwrap();
        assert!(p.b.list_documents(&x).unwrap().iter().any(|d| d.id == y));
        assert_eq!(p.b.previous_numbers(y), vec![x]);
        // On the computer that did not renumber anything: only its own numbers.
        sync(&mut p.a);
        assert!(p.a.previous_numbers(id_of(&p.a, "Registro de B")).is_empty());
    }

    #[test]
    fn a_creation_waiting_to_be_sent_has_a_provisional_number() {
        let mut p = pair();
        // Nothing waits for a library alone, nor for what came from the copy.
        let dir = tempfile::tempdir().unwrap();
        let mut alone = Store::open(&dir.path().join("alone.sqlite3")).unwrap();
        let (own, _) = create(&mut alone, "Registro solto", "Texto");
        assert!(!alone.load_document(own).unwrap().summary.provisional);
        assert!(!p.a.list_documents("").unwrap().iter().any(|d| d.provisional));

        // In the shared-folder mode the number is provisional until it is sent.
        let (a_doc, _) = create(&mut p.a, "Registro de A", "Texto de A");
        assert!(p.a.load_document(a_doc).unwrap().summary.provisional);
        // An edit while it waits does not change that, and the SIGADOC
        // document never is.
        p.a.update_manual_document(a_doc, &fields("Registro de A", "Texto novo"))
            .unwrap();
        let docs = p.a.list_documents("").unwrap();
        assert_eq!(docs.iter().filter(|d| d.provisional).count(), 1);
        assert!(docs.iter().filter(|d| d.provisional).all(|d| d.id == a_doc));
        assert!(docs.iter().filter(|d| !d.manual).all(|d| !d.provisional));
        sync(&mut p.a);
        assert!(!p.a.list_documents("").unwrap().iter().any(|d| d.provisional));
        // B receives it as final.
        sync(&mut p.b);
        assert!(!p.b.list_documents("").unwrap().iter().any(|d| d.provisional));
        // B's own record is provisional until B sends it.
        let (b_doc, _) = create(&mut p.b, "Registro de B", "Texto de B");
        assert!(p.b.load_document(b_doc).unwrap().summary.provisional);
        sync(&mut p.b);
        assert!(!p.b.list_documents("").unwrap().iter().any(|d| d.provisional));
    }
}
