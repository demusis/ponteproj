//! Development only (`--features shots`): `ponteproj --shots PASTA [FILTRO]`
//! opens the window on a sample library kept inside PASTA, goes through the
//! screens by itself and saves one picture of each. It never touches the real
//! library and needs neither mouse nor keyboard.
use anyhow::{Context, Result};
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

struct Request {
    out: PathBuf,
    filter: String,
}

static REQUEST: OnceLock<Request> = OnceLock::new();

/// Size of the window while the pictures are taken.
pub const SIZE: [f32; 2] = [1280.0, 800.0];

pub fn active() -> bool {
    REQUEST.get().is_some()
}

/// Creates the sample library inside `out` and points the application at it.
/// Call before the window opens.
pub fn prepare(out: PathBuf, filter: String) -> Result<()> {
    std::fs::create_dir_all(&out)?;
    let data = out.join("acervo");
    if data.exists() {
        std::fs::remove_dir_all(&data).context("Não foi possível limpar o acervo de exemplo")?;
    }
    std::fs::create_dir_all(&data)?;
    std::env::set_var("PONTEPROJ_DATA_DIR", &data);
    let mut store = crate::storage::Store::open(&crate::paths::db_path()?)?;
    crate::shots_data::seed(&mut store)?;
    drop(store);
    let _ = REQUEST.set(Request { out, filter });
    Ok(())
}

enum State {
    /// The next step to prepare.
    Next(usize),
    /// The screen is settling (lists load, layouts stabilise).
    Settling(u32),
    /// The picture was asked for.
    Capturing,
}

pub enum Tick {
    /// Prepare this step now (or say there is none).
    Begin(usize),
    Waiting,
}

pub struct Tour {
    state: State,
    index: usize,
    name: String,
    picture: Arc<Mutex<Option<Arc<egui::ColorImage>>>>,
}

impl Tour {
    pub fn start() -> Option<Self> {
        REQUEST.get().map(|_| Self {
            state: State::Next(0),
            index: 0,
            name: String::new(),
            picture: Default::default(),
        })
    }

    /// True when the step with this name is wanted (no filter, or it contains it).
    pub fn wanted(name: &str) -> bool {
        REQUEST
            .get()
            .is_none_or(|r| r.filter.is_empty() || name.contains(&r.filter))
    }

    /// The step being prepared is called `name`; it is pictured after a few frames.
    pub fn named(&mut self, index: usize, name: &str) {
        self.index = index;
        self.name = name.to_owned();
        self.state = State::Settling(0);
    }

    /// The step was skipped by the filter.
    pub fn skip(&mut self, index: usize) {
        self.state = State::Next(index + 1);
    }

    pub fn tick(&mut self, ctx: &egui::Context) -> Tick {
        match self.state {
            State::Next(index) => Tick::Begin(index),
            State::Settling(frames) => {
                if frames >= 12 {
                    let slot = self.picture.clone();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::ScreenshotCallback::new(move |image| {
                            if let Ok(mut slot) = slot.lock() {
                                *slot = Some(image);
                            }
                        }),
                    ));
                    self.state = State::Capturing;
                } else {
                    self.state = State::Settling(frames + 1);
                }
                Tick::Waiting
            }
            State::Capturing => {
                let image = self.picture.lock().ok().and_then(|mut slot| slot.take());
                if let Some(image) = image {
                    self.save(&image);
                    self.state = State::Next(self.index + 1);
                }
                Tick::Waiting
            }
        }
    }

    fn save(&self, image: &egui::ColorImage) {
        let Some(request) = REQUEST.get() else {
            return;
        };
        let path = request
            .out
            .join(format!("{:02}-{}.png", self.index, self.name));
        let [width, height] = image.size;
        let saved =
            image::RgbaImage::from_raw(width as u32, height as u32, image.as_raw().to_vec())
                .context("imagem vazia")
                .and_then(|picture| picture.save(&path).map_err(Into::into));
        match saved {
            Ok(()) => eprintln!("{}", path.display()),
            Err(error) => eprintln!("{}: {error}", path.display()),
        }
    }
}
