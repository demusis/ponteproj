//! Asks GitHub, only when the person clicks, whether a newer Ponte exists.
//! Nothing here runs by itself: no check at startup, no periodic check.
//!
//! The only request is a plain `GET` of the public "latest release" page of
//! the project. It carries a `User-Agent` (GitHub requires one) and an
//! `Accept` header, nothing else: no cookies, no credentials, no machine name.

use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::io::Read;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

const LATEST_API: &str = "https://api.github.com/repos/demusis/ponteproj/releases/latest";
const RELEASES_PREFIX: &str = "https://github.com/demusis/ponteproj/releases/";
/// Where the person is sent when the address in the answer is not trusted.
pub const FALLBACK_URL: &str = "https://github.com/demusis/ponteproj/releases/latest";
const MAX_BODY: u64 = 256 * 1024;
const MSG_UNDERSTAND: &str = "Não foi possível entender a resposta do GitHub.";

/// Result of one check, already worded for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    UpToDate {
        latest: String,
    },
    Newer {
        latest: String,
        /// Validated: always a page of the project's releases.
        url: String,
        /// `dd/mm/aaaa`, or empty when the answer had no usable date.
        published: String,
    },
    Failed(String),
}

/// What Ajustes ▸ Sobre o Ponte shows about the last check.
#[derive(Default)]
pub struct UpdateState {
    receiver: Option<Receiver<Outcome>>,
    pub last: Option<Outcome>,
    pub obtained: Option<Instant>,
}

impl UpdateState {
    /// True while a request is in flight.
    pub fn running(&self) -> bool {
        self.receiver.is_some()
    }
}

/// Starts one check in a background thread; does nothing if one is running.
pub fn start(state: &mut UpdateState, ctx: &eframe::egui::Context) {
    if state.receiver.is_some() {
        return;
    }
    let (tx, rx) = mpsc::channel();
    state.receiver = Some(rx);
    state.last = None;
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let outcome = check_remote(env!("CARGO_PKG_VERSION"));
        let _ = tx.send(outcome);
        ctx.request_repaint();
    });
}

/// Moves a finished result into the state, without blocking.
pub fn poll(state: &mut UpdateState) {
    let Some(rx) = &state.receiver else { return };
    match rx.try_recv() {
        Ok(outcome) => {
            state.last = Some(outcome);
            state.obtained = Some(Instant::now());
            state.receiver = None;
        }
        Err(TryRecvError::Empty) => {}
        Err(TryRecvError::Disconnected) => {
            state.last = Some(Outcome::Failed(MSG_UNDERSTAND.to_owned()));
            state.obtained = Some(Instant::now());
            state.receiver = None;
        }
    }
}

fn check_remote(installed: &str) -> Outcome {
    match fetch(installed) {
        Ok(body) => evaluate(installed, &body),
        Err(message) => Outcome::Failed(message),
    }
}

/// The one request. The error is already plain Portuguese.
fn fetch(installed: &str) -> std::result::Result<Vec<u8>, String> {
    const NO_CONNECTION: &str = "Sem conexão com a internet ou o GitHub não respondeu.";
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("Ponte/{installed}"))
        .build()
        .map_err(|_| NO_CONNECTION.to_owned())?;
    let response = client
        .get(LATEST_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|_| NO_CONNECTION.to_owned())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "O GitHub respondeu com um erro (código {}).",
            status.as_u16()
        ));
    }
    let mut body = Vec::new();
    response
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|_| NO_CONNECTION.to_owned())?;
    if body.len() as u64 > MAX_BODY {
        return Err(MSG_UNDERSTAND.to_owned());
    }
    Ok(body)
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
}

/// Compares the answer with the installed version.
fn evaluate(installed: &str, body: &[u8]) -> Outcome {
    let failed = || Outcome::Failed(MSG_UNDERSTAND.to_owned());
    let Ok(release) = serde_json::from_slice::<Release>(body) else {
        return failed();
    };
    let (Ok(latest), Ok(current)) = (parse_version(&release.tag_name), parse_version(installed))
    else {
        return failed();
    };
    let label = format!("{}.{}.{}", latest.0, latest.1, latest.2);
    if latest > current {
        Outcome::Newer {
            latest: label,
            url: safe_release_url(&release.html_url),
            published: release
                .published_at
                .as_deref()
                .map(format_date)
                .unwrap_or_default(),
        }
    } else {
        Outcome::UpToDate { latest: label }
    }
}

/// `0.28.0` or `v0.28.0` as a numeric triple.
fn parse_version(text: &str) -> Result<(u64, u64, u64)> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.split('.');
    let mut next = || -> Result<u64> {
        let part = parts.next().ok_or_else(|| anyhow!("version"))?;
        if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(anyhow!("version"));
        }
        Ok(part.parse()?)
    };
    let triple = (next()?, next()?, next()?);
    if parts.next().is_some() {
        return Err(anyhow!("version"));
    }
    Ok(triple)
}

/// Only pages of the project's releases are trusted; anything else becomes
/// the fixed releases page.
pub fn safe_release_url(url: &str) -> String {
    let ok = url.starts_with(RELEASES_PREFIX)
        && url[RELEASES_PREFIX.len()..]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
        && !url[RELEASES_PREFIX.len()..].contains("..");
    if ok {
        url.to_owned()
    } else {
        FALLBACK_URL.to_owned()
    }
}

/// `2026-11-08T12:00:00Z` to `08/11/2026`; empty when it does not fit.
fn format_date(iso: &str) -> String {
    let date = iso.get(..10).unwrap_or("");
    let mut parts = date.split('-');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(y), Some(m), Some(d))
            if y.len() == 4
                && m.len() == 2
                && d.len() == 2
                && [y, m, d]
                    .iter()
                    .all(|p| p.bytes().all(|b| b.is_ascii_digit())) =>
        {
            format!("{d}/{m}/{y}")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(tag: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/demusis/ponteproj/releases/tag/{tag}","published_at":"2026-11-08T12:30:00Z","extra":1}}"#
        )
    }

    #[test]
    fn versions_compare_numerically() {
        let v = |s| parse_version(s).unwrap();
        assert!(v("0.27.0") < v("0.28.0"));
        assert!(v("0.28.0") < v("0.100.0"));
        assert_eq!(v("v1.2.3"), (1, 2, 3));
        for bad in [
            "",
            "v",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "a.b.c",
            "1..3",
            "v1.2.-3",
            "1.2.3-rc1",
        ] {
            assert!(parse_version(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn newer_same_and_older() {
        match evaluate("0.27.0", body("v0.28.0").as_bytes()) {
            Outcome::Newer {
                latest,
                url,
                published,
            } => {
                assert_eq!(latest, "0.28.0");
                assert!(url.ends_with("/tag/v0.28.0"));
                assert_eq!(published, "08/11/2026");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            evaluate("0.28.0", body("v0.28.0").as_bytes()),
            Outcome::UpToDate {
                latest: "0.28.0".into()
            }
        );
        assert!(matches!(
            evaluate("0.28.0", body("v0.27.0").as_bytes()),
            Outcome::UpToDate { .. }
        ));
    }

    #[test]
    fn malformed_answers_fail_plainly() {
        let expected = Outcome::Failed(MSG_UNDERSTAND.to_owned());
        for bad in [
            "",
            "not json",
            "[]",
            r#"{"html_url":"https://github.com/demusis/ponteproj/releases/x"}"#,
            r#"{"tag_name":"v0.28.0"}"#,
            r#"{"tag_name":"nightly","html_url":"x"}"#,
        ] {
            assert_eq!(evaluate("0.27.0", bad.as_bytes()), expected, "{bad}");
        }
    }

    #[test]
    fn missing_date_is_tolerated() {
        let json = r#"{"tag_name":"v0.28.0","html_url":"https://github.com/demusis/ponteproj/releases/tag/v0.28.0"}"#;
        match evaluate("0.27.0", json.as_bytes()) {
            Outcome::Newer { published, .. } => assert!(published.is_empty()),
            other => panic!("{other:?}"),
        }
        assert_eq!(format_date("garbage"), "");
        assert_eq!(format_date("2026-11-08"), "08/11/2026");
    }

    #[test]
    fn only_project_release_urls_are_trusted() {
        let good = "https://github.com/demusis/ponteproj/releases/tag/v0.28.0";
        assert_eq!(safe_release_url(good), good);
        for bad in [
            "http://github.com/demusis/ponteproj/releases/tag/v1",
            "https://github.com/outro/ponteproj/releases/tag/v1",
            "https://github.com.evil.example/demusis/ponteproj/releases/x",
            "https://github.com/demusis/ponteproj/releases/../../x",
            "https://github.com/demusis/ponteproj/releases/%2e%2e/%2e%2e/outro/repo",
            "https://github.com/demusis/ponteproj/releases/tag/v1?x=1",
            "https://github.com/demusis/ponteproj/releases/a b",
            "javascript:alert(1)",
            "",
        ] {
            assert_eq!(safe_release_url(bad), FALLBACK_URL, "{bad}");
        }
    }
}
