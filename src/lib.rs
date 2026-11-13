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
pub mod attachments;
pub mod chatgpt;
/// Deterministic checks of a draft (Conferência da minuta).
pub mod draft_check;
pub mod export;
pub mod files;
pub mod guide;
#[cfg(feature = "desktop")]
pub mod help_ui;
pub mod idmap;
pub mod imap;
pub mod mail;
#[cfg(feature = "desktop")]
pub mod mail_ui;
pub mod manual;
pub mod model;
pub mod native;
pub mod paths;
pub mod prompting;
pub mod protection;
/// A record with its attachments created in one operation (all or nothing).
pub mod record_import;
pub mod sharedfolder;
pub mod storage;
pub mod suggestion;
pub mod tasks;
#[cfg(feature = "desktop")]
pub mod tasks_ui;
pub mod template_bundle;
#[cfg(feature = "desktop")]
pub mod templates_ui;
/// Messages copied from a WhatsApp conversation, read into a record.
pub mod whatsapp;
pub mod work_context;
pub mod work_templates;

pub mod dossier;

pub mod pdf_reader;

pub mod evidence;
pub mod extraction;

pub mod draft_tools;

pub mod trash;

pub mod intranet;

pub mod saiop;

/// Rooms, books and equipment, and their bookings and loans.
pub mod resources;

#[cfg(feature = "desktop")]
pub mod icon;

pub mod labels;

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
