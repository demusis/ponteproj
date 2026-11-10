//! The Ponte library crate: every module of the program. `main.rs` only
//! decides whether to open the window, serve the browser extension or run a
//! command-line option.
pub mod ai;
#[cfg(feature = "desktop")]
pub mod app;
pub mod attachments;
pub mod chatgpt;
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
pub mod sharedfolder;
pub mod storage;
pub mod suggestion;
pub mod tasks;
#[cfg(feature = "desktop")]
pub mod tasks_ui;
pub mod template_bundle;
#[cfg(feature = "desktop")]
pub mod templates_ui;
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

#[cfg(feature = "desktop")]
pub mod icon;

pub mod labels;

#[cfg(feature = "desktop")]
pub mod windows_ui;

#[cfg(feature = "desktop")]
pub mod theme;
#[cfg(feature = "desktop")]
pub mod widgets;

/// Sample library for the pictures of the screens (development only).
#[cfg(any(test, feature = "shots"))]
pub mod shots_data;
#[cfg(feature = "shots")]
pub mod shots;
