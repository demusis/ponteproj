//! The Ponte library crate: every module of the program. `main.rs` only
//! decides whether to open the window, serve the browser extension or run a
//! command-line option.
pub mod ai;
pub mod app;
pub mod attachments;
pub mod chatgpt;
pub mod export;
pub mod files;
pub mod guide;
pub mod idmap;
pub mod manual;
pub mod model;
pub mod native;
pub mod paths;
pub mod prompting;
pub mod protection;
pub mod sharedfolder;
pub mod storage;
pub mod tasks;
pub mod tasks_ui;
pub mod templates_ui;
pub mod work_context;
pub mod work_templates;

pub mod dossier;

pub mod pdf_reader;

pub mod evidence;

pub mod draft_tools;

pub mod trash;

pub mod intranet;

pub mod saiop;

pub mod icon;

pub mod labels;

pub mod windows_ui;
