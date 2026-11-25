//! What a form read and what is saved now. A version number alone never
//! proves that nobody else changed a record: two devices that start from
//! the same number and save the same number of times reach the same number
//! with different contents. A form that holds the record it read compares
//! it with the one saved now before it writes: the same content means only
//! the version moved (the save goes on the current version); another
//! content means a real edit elsewhere (nothing is written, the typing
//! stays, and the form says so).

use serde::{de::DeserializeOwned, Serialize};

use crate::intranet::VersionMeta;

/// The fields the store sets on every save of a record (when it was made
/// and when it was saved, who made it): never what a form edits, and
/// rewritten by every replay of the record on another device.
pub const STAMPS: &[&str] = &["created_at", "updated_at", "created_by"];

/// The fields of `value`, as JSON, without those in `skip`.
pub fn fields_of<T: Serialize>(
    value: &T,
    skip: &[&str],
) -> serde_json::Map<String, serde_json::Value> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::Object(mut map)) => {
            for key in skip {
                map.remove(*key);
            }
            map
        }
        _ => serde_json::Map::new(),
    }
}

/// Whether `a` and `b` say the same thing (the fields in `skip` aside).
pub fn same_record<T: Serialize>(a: &T, b: &T, skip: &[&str]) -> bool {
    fields_of(a, skip) == fields_of(b, skip)
}

/// The fields in which `b` differs from `a` (the fields in `skip` aside).
pub fn changed_keys<T: Serialize>(a: &T, b: &T, skip: &[&str]) -> Vec<String> {
    let (a, b) = (fields_of(a, skip), fields_of(b, skip));
    let mut out: Vec<String> = Vec::new();
    for key in a.keys().chain(b.keys()) {
        if a.get(key) != b.get(key) && !out.contains(key) {
            out.push(key.clone());
        }
    }
    out
}

/// The three-way merge of a form: from `base` (what the form read), every
/// field the person did not change takes the value saved now (`theirs`);
/// a field the person changed keeps the person's value. The fields in
/// `skip` take the value saved now. Gives the merged record and the fields
/// both sides changed to different values (the person's value was kept
/// there, and the other must be shown). `None` when the merged record
/// cannot be read back.
pub fn merge_three<T: Serialize + DeserializeOwned>(
    base: &T,
    theirs: &T,
    mine: &T,
    skip: &[&str],
) -> Option<(T, Vec<String>)> {
    let (b, t, m) = (
        fields_of(base, &[]),
        fields_of(theirs, &[]),
        fields_of(mine, &[]),
    );
    let mut merged = serde_json::Map::new();
    let mut both = Vec::new();
    let keys: std::collections::BTreeSet<&String> =
        b.keys().chain(t.keys()).chain(m.keys()).collect();
    for key in keys {
        let (bv, tv, mv) = (b.get(key), t.get(key), m.get(key));
        let chosen = if skip.contains(&key.as_str()) || mv == bv {
            tv
        } else {
            if tv != bv && tv != mv {
                both.push(key.clone());
            }
            mv
        };
        if let Some(value) = chosen {
            merged.insert(key.clone(), value.clone());
        }
    }
    let record: T = serde_json::from_value(serde_json::Value::Object(merged)).ok()?;
    Some((record, both))
}

/// Who saved the version found and when, said so that it makes sense when
/// it is the person's own name (the same on every computer and on the
/// phone): then it was another computer, the phone or another screen.
pub fn saved_by(meta: &VersionMeta, me: &str) -> String {
    let when = crate::labels::datetime(&meta.updated_at);
    let name = meta.updated_by.trim();
    let who = if name.is_empty() {
        "por outra pessoa".to_owned()
    } else if name.eq_ignore_ascii_case(me.trim()) {
        format!("com o seu nome ({name}) em outro computador, no celular ou em outra tela")
    } else {
        format!("por {name}")
    };
    if when.trim().is_empty() {
        who
    } else {
        format!("{who}, em {when}")
    }
}

/// The refusal of a form whose record changed after it was read, whatever
/// its version number: `subject` says what changed ("Este projeto foi
/// alterado"), `way_out` what the person can do now.
pub fn changed_since_read(subject: &str, meta: &VersionMeta, me: &str, way_out: &str) -> String {
    changed_then(
        subject,
        meta,
        me,
        "Nada foi gravado e o que você digitou continua aqui.",
        way_out,
    )
}

/// The refusal of a delete asked from a screen whose record changed after
/// it was read, whatever its version number: nothing is deleted, and the
/// person decides again on what is saved now.
pub fn changed_before_delete(subject: &str, meta: &VersionMeta, me: &str, way_out: &str) -> String {
    changed_then(subject, meta, me, "Nada foi apagado.", way_out)
}

/// The refusal of a screen whose record changed after it was read: `done`
/// says what was (not) done, `way_out` what the person can do now.
pub fn changed_then(
    subject: &str,
    meta: &VersionMeta,
    me: &str,
    done: &str,
    way_out: &str,
) -> String {
    format!(
        "Conflito de edição simultânea: {subject} {} depois que você abriu esta tela (versão #{}). {done} {way_out}",
        saved_by(meta, me),
        meta.version
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
    struct Card {
        name: String,
        notes: String,
        updated_at: String,
    }

    fn card(name: &str, notes: &str, at: &str) -> Card {
        Card {
            name: name.into(),
            notes: notes.into(),
            updated_at: at.into(),
        }
    }

    #[test]
    fn the_stamps_never_count_as_a_change() {
        let read = card("A", "x", "2026-11-24T10:00:00Z");
        let replayed = card("A", "x", "2026-11-24T11:00:00Z");
        assert!(same_record(&read, &replayed, STAMPS));
        assert!(!same_record(&read, &card("A", "y", ""), STAMPS));
        assert_eq!(changed_keys(&read, &card("B", "x", ""), STAMPS), ["name"]);
    }

    #[test]
    fn the_merge_keeps_the_typing_and_brings_the_rest() {
        let base = card("A", "x", "1");
        let theirs = card("A", "lá", "2");
        let mine = card("meu", "x", "1");
        let (merged, both) = merge_three(&base, &theirs, &mine, STAMPS).unwrap();
        assert_eq!(merged, card("meu", "lá", "2"));
        assert!(both.is_empty());
        let mine = card("A", "meu", "1");
        let (merged, both) = merge_three(&base, &theirs, &mine, STAMPS).unwrap();
        assert_eq!(merged.notes, "meu");
        assert_eq!(both, ["notes"]);
    }
}
