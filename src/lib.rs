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
pub mod chatgpt;
/// Deterministic checks of a draft (Conferência da minuta).
pub mod draft_check;
pub mod export;
pub mod files;
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
pub mod paths;
pub mod plan_import;
pub mod projection;
pub mod prompting;
pub mod protection;
/// A record with its attachments created in one operation (all or nothing).
pub mod record_import;
pub mod risk;
pub mod sharedfolder;
pub mod storage;
pub mod suggestion;
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

pub mod dossier;

pub mod pdf_reader;

pub mod evidence;
pub mod extraction;

pub mod draft_tools;

pub mod trash;

pub mod intranet;

pub mod saiop;

pub mod calendar_ics;
/// Categories the person defines, for tasks and documents.
pub mod categories;
/// Categories on the screens: chips, picker, filter and manager.
#[cfg(feature = "desktop")]
pub mod category_ui;
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
