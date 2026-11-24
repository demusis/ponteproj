//! The Ponte library crate: every module of the program. `main.rs` only
//! decides whether to open the window, serve the browser extension or run a
//! command-line option.
// The development-only screen tour appends its hooks at the end of the files.
#![cfg_attr(feature = "shots", allow(clippy::items_after_test_module))]
pub mod ai;
/// The set of fictitious cases that measures the roles.
pub mod ai_eval;
/// The Extrator and the Propositor of the document analysis.
pub mod ai_roles;
#[cfg(feature = "desktop")]
pub mod app;
pub mod atomic_file;
pub mod attachments;
/// The automatic copy of the day and "Fazer cópia agora", with their retention.
pub mod auto_backup;
pub mod chatgpt;
/// Deterministic checks of a draft (Conferência da minuta).
pub mod draft_check;
pub mod export;
pub mod files;
/// Size of the shared file and the room left, copies the cloud made beside it, and what never syncs.
pub mod folder_health;
pub mod guide;
#[cfg(feature = "desktop")]
pub mod help_ui;
/// Brazilian national holidays (non-working days of the planning).
pub mod holidays;
pub mod idmap;
pub mod imap;
pub mod impact;
pub mod mail;
#[cfg(feature = "desktop")]
pub mod mail_ui;
pub mod manual;
pub mod model;
pub mod native;
/// How the program reaches the internet: certificates, proxy, failures, the connection test.
pub mod netdiag;
/// "Exportar tudo em pasta aberta…": the whole library as open files (Markdown and one JSON).
pub mod open_export;
pub mod paths;
/// `--import-plan`: a record and its tasks from one JSON file (the desktop command line).
#[cfg(feature = "desktop")]
pub mod plan_import;
/// "Cópia portátil (com senha)": the library in one `.pontecopia` file.
pub mod portable_copy;
pub mod projection;
pub mod prompting;
pub mod protection;
/// A record with its attachments created in one operation (all or nothing).
pub mod record_import;
/// "Restaurar cópia…": recovery libraries in folders of their own, and the
/// choice of the library in use.
pub mod recovery;
/// The notice of a manual record whose number changed on sync, the old
/// number kept findable and the provisional marker.
pub mod renumber;
pub mod risk;
pub mod sharedfolder;
pub mod storage;
pub mod suggestion;
#[cfg(feature = "desktop")]
pub mod task_filters;
pub mod tasks;
#[cfg(feature = "desktop")]
pub mod tasks_ui;
pub mod template_bundle;
#[cfg(feature = "desktop")]
pub mod templates_ui;
/// The safety copy of the library that the installers make before an update.
pub mod upgrade_backup;
/// Messages copied from a WhatsApp conversation, read into a record.
pub mod whatsapp;
pub mod work_context;
pub mod work_templates;
pub mod workload;

/// F1 of Frente F: characterization tests of row ids (tests only).
#[cfg(test)]
mod f1_inventory;
#[cfg(test)]
mod f2a_parity;
#[cfg(test)]
mod f2a_review2_tests;
/// F2a: the review regression tests and the parity with the build without F2a.
#[cfg(test)]
mod f2a_review_tests;
/// F2a of Frente F: acceptance tests of the library schema 22 (tests only).
#[cfg(test)]
mod f2a_tests;
/// F2b: the parity machinery of F2a extended to the shared file of format 9.
#[cfg(test)]
mod f2b_parity;
/// F2b: one regression test per finding of its review.
#[cfg(test)]
mod f2b_review_tests;
/// F2b: one regression test per finding of its second review.
#[cfg(test)]
mod f2b_rodada2_tests;
/// F2b of Frente F: acceptance tests of the shared file of format 9.
#[cfg(test)]
mod f2b_tests;
/// F3.1a-ii of Frente F: acceptance tests of the record kinds addressed by
/// identity (tests only).
#[cfg(test)]
mod f31a2_tests;
/// Review of F3.1a-i: the moment of a new version, the pointers after a
/// replay and an equal snapshot, the oracle of the note rule and what Ponte
/// 0.51 showed kept by the update (tests only).
#[cfg(test)]
mod f31a_review_tests;
/// F3.1a of Frente F: acceptance tests of the comparator of versions, the
/// pointers on final identities and the obligations of the record kinds
/// (tests only).
#[cfg(test)]
mod f31a_tests;
/// Review of F3.1b1: the epoch of the copy being built, an edit from another
/// epoch as a card, and the paths of risk the review listed (tests only).
#[cfg(test)]
mod f31b1_review_tests;
/// F3.1b1 of Frente F: acceptance tests of the manual target bound inside
/// the import's transaction, the request id of another record and the
/// native host on a crossed pair (tests only).
#[cfg(test)]
mod f31b1_tests;
/// F3.1b2 of Frente F: acceptance tests of the deletion proved by a
/// same-epoch adoption and of the library space (tests only).
#[cfg(test)]
mod f31b2_tests;
/// F3.0a1 of Frente F: acceptance tests of the level barrier, the floor
/// observed and the refs of every producer (tests only).
#[cfg(test)]
mod f3_tests;
/// F3.0a2 of Frente F: acceptance tests of the resolution of the refs, the
/// host's durable insertion log and the receipt asked first (tests only).
#[cfg(test)]
mod f3a2_tests;
/// Review of F3.0b-ii: the card of a foreign change in one frame and the
/// reservation of the mapper (tests only).
#[cfg(test)]
mod f3b2_review_tests;
/// F3.0b-ii of Frente F: acceptance tests of the adoption mapping, the
/// retired sources, coverage and report by identity and the card envelope
/// (tests only).
#[cfg(test)]
mod f3b2_tests;
/// Review of F3.0b-i: the try of the settle, the libraries left at schema
/// 21 and the card of the folder route (tests only).
#[cfg(test)]
mod f3b_review_tests;
/// F3.0b-i of Frente F: acceptance tests of the activation of the mapping,
/// the id floor kept by the copy and the translation of the answers (tests
/// only).
#[cfg(test)]
mod f3b_tests;
/// The minimum format of each shared folder this device has seen (F2b).
pub mod formatfloor;
/// Identities of the items (library schema 22).
pub mod idents;
/// The id sites of a change and the proof of each (F3).
pub mod idrefs;
/// The adoption mapping of a mapped library (F3.0b).
pub mod mapper;
/// Pacote F2 + F3 (0.52.0): the features of 0.51.0 that create or edit
/// records, under the rules of F3 (tests only).
#[cfg(test)]
mod pacote_tests;
/// A task form open during a folder exchange: the version it expects and
/// the question of the unsaved task (tests only).
#[cfg(test)]
mod tarefa_tests;

pub mod dossier;

pub mod pdf_reader;

pub mod evidence;
pub mod extraction;

pub mod draft_tools;

pub mod trash;

pub mod intranet;
/// A file the phone app took for a record: proved, or kept in a card.
pub mod phoneops;

pub mod saiop;

pub mod calendar_ics;
/// Categories the person defines, for tasks and documents.
pub mod categories;
/// Categories on the screens: chips, picker, filter and manager.
#[cfg(feature = "desktop")]
pub mod category_ui;
pub(crate) mod coverage;
/// Projects on the task screens: chip, picker and filter.
#[cfg(feature = "desktop")]
pub mod project_ui;
pub mod recurrence;
/// Rooms, books and equipment, and their bookings and loans.
pub mod resources;

#[cfg(feature = "desktop")]
pub mod icon;

pub mod labels;
/// Small named texts that belong to the library and sync with it.
pub mod library_settings;
/// Projects: an optional grouping of tasks, with progress, risk and timeline.
pub mod projects;

#[cfg(feature = "desktop")]
pub mod windows_ui;

#[cfg(feature = "desktop")]
pub mod reminders;

#[cfg(feature = "desktop")]
pub mod theme;
#[cfg(feature = "desktop")]
pub mod widgets;

#[cfg(feature = "shots")]
pub mod shots;
/// Sample library for the pictures of the screens (development only).
#[cfg(any(test, feature = "shots"))]
pub mod shots_data;

/// Choosing a document by searching for it.
#[cfg(feature = "desktop")]
pub mod doc_picker;

/// The "Recursos" area of the window.
#[cfg(feature = "desktop")]
pub mod resources_ui;

/// One window per library.
#[cfg(feature = "desktop")]
pub mod single_instance;

/// The ficha as a printable page.
pub mod ficha_export;

/// "Is there a newer version?", asked only on request.
#[cfg(feature = "desktop")]
pub mod update_check;
