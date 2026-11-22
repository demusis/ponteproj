//! Manual records change number when a creation made offline (or in a
//! shared folder) is replayed onto the shared copy. This module holds the
//! local notice of that change and the tests of the whole behavior.

#[cfg(test)]
mod tests {
    use crate::intranet::{IntranetConfig, IntranetMode};
    use crate::manual::ManualFields;
    use crate::storage::Store;

    fn folder_config(member: &str, files: Vec<String>) -> IntranetConfig {
        IntranetConfig {
            mode: IntranetMode::SharedFolder,
            member_name: member.into(),
            folder_files: files,
            folder_key: "senha-de-teste-123".into(),
            ..IntranetConfig::default()
        }
    }

    pub(super) fn fields(title: &str, text: &str) -> ManualFields {
        ManualFields {
            kind: "ligacao".into(),
            title: title.into(),
            origin: "Gabinete".into(),
            document_date: String::new(),
            text: text.into(),
        }
    }

    /// Two computers sharing one folder, both already in sync on an empty
    /// library (the copy exists and each one has adopted it).
    pub(super) struct Pair {
        pub _dir: tempfile::TempDir,
        pub a: Store,
        pub b: Store,
    }

    const KEY: &str = "senha-de-teste-123";

    pub(super) fn sync(store: &mut Store) -> crate::intranet::FolderSyncReport {
        let cfg = store.load_intranet_config().unwrap();
        store
            .sync_with_folders(&cfg, KEY, false, &crate::ai::Reporter::silent())
            .unwrap()
    }

    pub(super) fn pair() -> Pair {
        let dir = tempfile::tempdir().unwrap();
        let drive = dir.path().join("Drive").join("ponte-sync.ponte");
        std::fs::create_dir_all(drive.parent().unwrap()).unwrap();
        let files = vec![drive.display().to_string()];
        let mut a = Store::open(&dir.path().join("a.sqlite3")).unwrap();
        a.save_intranet_config(&folder_config("Casa", files.clone()))
            .unwrap();
        // A document of the SIGADOC makes the first copy a real one.
        a.import_capture(&crate::model::sample_capture()).unwrap();
        sync(&mut a);
        let mut b = Store::open(&dir.path().join("b.sqlite3")).unwrap();
        b.save_intranet_config(&folder_config("Trabalho", files))
            .unwrap();
        sync(&mut b);
        Pair { _dir: dir, a, b }
    }

    pub(super) fn number_of(store: &Store, title: &str) -> Option<String> {
        store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.title == title)
            .map(|d| d.document_number)
    }

    pub(super) fn text_of(store: &Store, number: &str) -> String {
        let id = store
            .list_documents("")
            .unwrap()
            .into_iter()
            .find(|d| d.document_number == number)
            .unwrap_or_else(|| panic!("sem {number}"))
            .id;
        store.load_document(id).unwrap().capture.text
    }

    /// The defect: B creates a manual record offline and edits it before the
    /// first exchange; the shared copy already has another record with the
    /// same number. B's creation is numbered again on replay, and its edit,
    /// addressed by the old number, must follow it, never land on the other
    /// record.
    #[test]
    fn an_edit_of_a_renumbered_creation_never_lands_on_the_record_that_took_its_number() {
        let mut p = pair();
        // A publishes its record first.
        let x = p
            .a
            .create_manual_document(&fields("Registro de A", "Texto de A"))
            .unwrap();
        let x_number = p.a.load_document(x.document_id).unwrap().summary.document_number;
        sync(&mut p.a);
        // B, still without that copy, creates and edits its own record.
        let y = p
            .b
            .create_manual_document(&fields("Registro de B", "Texto de B"))
            .unwrap();
        let y_number = p.b.load_document(y.document_id).unwrap().summary.document_number;
        assert_eq!(x_number, y_number, "the two libraries numbered alike");
        p.b.update_manual_document(y.document_id, &fields("Registro de B", "Texto de B, editado"))
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
}
