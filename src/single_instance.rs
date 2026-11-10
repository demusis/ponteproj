//! One window per library. A second start hands its request (open this
//! document, or just come to the front) to the window already open and
//! leaves, instead of opening another window over the same library.
//!
//! The first window keeps `janela.lock` open without sharing; a later start
//! cannot open it, writes `janela-pedido.txt` and waits for the window to
//! take it. The request holds an action and a local id, never content.
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LOCK_FILE: &str = "janela.lock";
const REQUEST_FILE: &str = "janela-pedido.txt";
/// How long a second start waits for the open window to take the request
/// before opening a window of its own, as older versions did.
const HANDOFF_WAIT: Duration = Duration::from_secs(4);

/// What a start asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// Come to the front.
    Focus,
    /// Come to the front showing this document.
    Document(i64),
    /// Come to the front on the Modelos area.
    Templates,
}

impl Request {
    fn encode(self) -> String {
        match self {
            Self::Focus => "janela".to_owned(),
            Self::Document(id) => format!("documento {id}"),
            Self::Templates => "modelos".to_owned(),
        }
    }

    fn decode(text: &str) -> Option<Self> {
        let mut words = text.split_whitespace();
        let request = match (words.next()?, words.next()) {
            ("janela", None) => Self::Focus,
            ("modelos", None) => Self::Templates,
            ("documento", Some(id)) => Self::Document(id.parse().ok().filter(|id| *id > 0)?),
            _ => return None,
        };
        words.next().is_none().then_some(request)
    }
}

/// Held by the window for as long as it lives; the system releases it when
/// the process ends, however it ends.
pub struct Guard {
    _lock: File,
}

fn open_exclusive(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // No sharing: a second open fails while this handle lives.
        options.share_mode(0);
    }
    options.open(path)
}

/// The window of this library, when no other is open.
pub fn acquire(data_dir: &Path) -> Option<Guard> {
    open_exclusive(&data_dir.join(LOCK_FILE))
        .ok()
        .map(|lock| Guard { _lock: lock })
}

fn request_path(data_dir: &Path) -> PathBuf {
    data_dir.join(REQUEST_FILE)
}

/// Leaves the request for the open window and waits for it to be taken.
/// False when nobody took it (the caller then opens its own window).
pub fn hand_over(data_dir: &Path, request: Request) -> bool {
    let path = request_path(data_dir);
    let temporary = data_dir.join(format!("{REQUEST_FILE}.{}", std::process::id()));
    if std::fs::write(&temporary, request.encode()).is_err()
        || std::fs::rename(&temporary, &path).is_err()
    {
        let _ = std::fs::remove_file(&temporary);
        return false;
    }
    let started = Instant::now();
    while started.elapsed() < HANDOFF_WAIT {
        if !path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(&path);
    false
}

/// The request another start left, if any; taking it removes it.
pub fn take_request(data_dir: &Path) -> Option<Request> {
    let path = request_path(data_dir);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    Request::decode(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_survive_the_file_and_garbage_is_refused() {
        for request in [Request::Focus, Request::Templates, Request::Document(42)] {
            assert_eq!(Request::decode(&request.encode()), Some(request));
        }
        for bad in [
            "",
            "documento",
            "documento x",
            "documento -3",
            "documento 0",
            "documento 1 2",
            "janela 1",
            "outra coisa",
        ] {
            assert_eq!(Request::decode(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_request_is_taken_once() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(take_request(dir.path()), None);
        std::fs::write(request_path(dir.path()), Request::Document(7).encode()).unwrap();
        assert_eq!(take_request(dir.path()), Some(Request::Document(7)));
        assert_eq!(take_request(dir.path()), None);
    }

    #[cfg(windows)]
    #[test]
    fn only_one_window_holds_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let first = acquire(dir.path());
        assert!(first.is_some());
        assert!(acquire(dir.path()).is_none());
        drop(first);
        assert!(acquire(dir.path()).is_some());
    }

    #[test]
    fn an_unanswered_request_is_withdrawn() {
        // Nobody takes it: the caller is told, and nothing is left behind.
        let dir = tempfile::tempdir().unwrap();
        let taker = {
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || {
                let started = Instant::now();
                while started.elapsed() < Duration::from_secs(3) {
                    if let Some(request) = take_request(&path) {
                        return Some(request);
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                None
            })
        };
        assert!(hand_over(dir.path(), Request::Focus));
        assert_eq!(taker.join().unwrap(), Some(Request::Focus));
        assert!(!request_path(dir.path()).exists());
    }
}
