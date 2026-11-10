//! The look of the window in one place: colours (light and dark), fonts, text
//! sizes, spacing and the icons. Screens never write a colour or a size of
//! their own; they ask this module.
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Theme,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The colours of one theme. Soft colours are backgrounds for the strong
/// colour of the same name (pills, selected rows).
pub struct Palette {
    /// Window background, behind the cards.
    pub bg: Color32,
    /// Cards, lists, fields.
    pub surface: Color32,
    /// Slightly darker than the surface: neutral pills, hovered rows.
    pub sunken: Color32,
    pub outline: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub primary: Color32,
    pub on_primary: Color32,
    pub primary_soft: Color32,
    pub primary_hover: Color32,
    pub rail: Color32,
    pub rail_text: Color32,
    pub rail_on: Color32,
    pub rail_selected: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub warn: Color32,
    pub warn_soft: Color32,
    pub ok: Color32,
    pub ok_soft: Color32,
    pub info: Color32,
    pub info_soft: Color32,
    pub violet: Color32,
    pub violet_soft: Color32,
    /// Floating notice ("Tarefa concluída").
    pub toast: Color32,
    pub toast_text: Color32,
    pub toast_action: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub static LIGHT: Palette = Palette {
    bg: rgb(243, 245, 246),
    surface: rgb(255, 255, 255),
    sunken: rgb(234, 238, 240),
    outline: rgb(213, 219, 223),
    text: rgb(27, 37, 41),
    muted: rgb(86, 100, 107),
    primary: rgb(24, 84, 97),
    on_primary: rgb(255, 255, 255),
    primary_soft: rgb(220, 235, 238),
    primary_hover: rgb(18, 68, 79),
    rail: rgb(24, 84, 97),
    rail_text: rgb(207, 226, 230),
    rail_on: rgb(255, 255, 255),
    rail_selected: rgb(58, 111, 122),
    danger: rgb(168, 40, 45),
    danger_soft: rgb(251, 233, 234),
    warn: rgb(138, 75, 0),
    warn_soft: rgb(253, 240, 217),
    ok: rgb(20, 108, 67),
    ok_soft: rgb(225, 243, 233),
    info: rgb(26, 95, 180),
    info_soft: rgb(226, 236, 250),
    violet: rgb(88, 64, 138),
    violet_soft: rgb(237, 232, 248),
    toast: rgb(27, 37, 41),
    toast_text: rgb(255, 255, 255),
    toast_action: rgb(143, 211, 224),
};

pub static DARK: Palette = Palette {
    bg: rgb(17, 25, 28),
    surface: rgb(26, 36, 40),
    sunken: rgb(34, 46, 51),
    outline: rgb(53, 69, 76),
    text: rgb(230, 236, 238),
    muted: rgb(163, 177, 183),
    primary: rgb(124, 196, 211),
    on_primary: rgb(11, 34, 40),
    primary_soft: rgb(31, 58, 65),
    primary_hover: rgb(150, 212, 225),
    rail: rgb(12, 20, 23),
    rail_text: rgb(163, 177, 183),
    rail_on: rgb(230, 236, 238),
    rail_selected: rgb(31, 58, 65),
    danger: rgb(242, 165, 168),
    danger_soft: rgb(74, 32, 35),
    warn: rgb(240, 192, 120),
    warn_soft: rgb(74, 54, 19),
    ok: rgb(134, 211, 168),
    ok_soft: rgb(22, 56, 42),
    info: rgb(156, 194, 245),
    info_soft: rgb(27, 48, 80),
    violet: rgb(198, 181, 240),
    violet_soft: rgb(46, 38, 72),
    toast: rgb(230, 236, 238),
    toast_text: rgb(17, 25, 28),
    toast_action: rgb(24, 84, 97),
};

static DARK_NOW: AtomicBool = AtomicBool::new(false);

/// The colours of the theme on screen.
pub fn c() -> &'static Palette {
    if DARK_NOW.load(Ordering::Relaxed) {
        &DARK
    } else {
        &LIGHT
    }
}

pub fn is_dark() -> bool {
    DARK_NOW.load(Ordering::Relaxed)
}

// The names the screens already used, now following the theme.
pub fn accent() -> Color32 {
    c().primary
}
pub fn warning() -> Color32 {
    c().warn
}
pub fn error() -> Color32 {
    c().danger
}
pub fn success() -> Color32 {
    c().ok
}
pub fn muted() -> Color32 {
    c().muted
}
pub fn on_time() -> Color32 {
    c().info
}
pub fn on_time_bg() -> Color32 {
    c().info_soft
}

/// Spacing steps. Screens use these instead of loose numbers.
pub const S1: f32 = 4.0;
pub const S2: f32 = 8.0;
pub const S3: f32 = 12.0;
pub const S4: f32 = 16.0;
pub const S5: f32 = 24.0;

/// Text sizes.
pub const SIZE_SMALL: f32 = 12.5;
pub const SIZE_BODY: f32 = 14.0;
pub const SIZE_SECTION: f32 = 15.0;
pub const SIZE_HEADING: f32 = 20.0;
pub const SIZE_TITLE: f32 = 22.0;

pub const RADIUS: u8 = 6;
pub const RADIUS_CARD: u8 = 10;

/// The heavier weight used for titles, buttons and pills.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn font_strong(size: f32) -> FontId {
    FontId::new(size, semibold())
}

/// What the person chose in Ajustes › Aparência.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

impl Appearance {
    pub fn key(self) -> &'static str {
        match self {
            Self::System => "sistema",
            Self::Light => "claro",
            Self::Dark => "escuro",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "claro" => Self::Light,
            "escuro" => Self::Dark,
            _ => Self::System,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Como o Windows",
            Self::Light => "Claro",
            Self::Dark => "Escuro",
        }
    }
}

/// How big everything is drawn, chosen in Ajustes ▸ Aparência.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextSize {
    Normal,
    Large,
    Larger,
}

impl TextSize {
    pub const ALL: [Self; 3] = [Self::Normal, Self::Large, Self::Larger];

    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Large => "grande",
            Self::Larger => "maior",
        }
    }

    pub fn from_key(key: &str) -> Self {
        match key {
            "grande" => Self::Large,
            "maior" => Self::Larger,
            _ => Self::Normal,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Large => "Grande",
            Self::Larger => "Maior",
        }
    }

    fn factor(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Large => 1.12,
            Self::Larger => 1.25,
        }
    }
}

/// Texts, icons and spacing grow together, like the zoom of a browser.
pub fn set_text_size(ctx: &egui::Context, size: TextSize) {
    ctx.set_zoom_factor(size.factor());
}

fn windows_font(file: &str) -> Option<Vec<u8>> {
    let windows = std::env::var_os("WINDIR")?;
    std::fs::read(PathBuf::from(windows).join("Fonts").join(file)).ok()
}

/// Segoe UI from Windows for the text (the bundled font stays as reserve), a
/// heavier cut for titles, the icon font and Segoe UI Symbol for rare glyphs.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: Vec<u8>| {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
    };
    let reserve: Vec<String> = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut regular = Vec::new();
    let mut strong = Vec::new();
    if let Some(bytes) = windows_font("segoeui.ttf") {
        add(&mut fonts, "segoe-ui", bytes);
        regular.push("segoe-ui".to_owned());
    }
    if let Some(bytes) = windows_font("seguisb.ttf") {
        add(&mut fonts, "segoe-ui-semibold", bytes);
        strong.push("segoe-ui-semibold".to_owned());
    }
    add(
        &mut fonts,
        "phosphor",
        include_bytes!("../assets/fonts/Phosphor.ttf").to_vec(),
    );
    let mut tail = reserve.clone();
    tail.push("phosphor".to_owned());
    if let Some(bytes) = windows_font("seguisym.ttf") {
        add(&mut fonts, "segoe-ui-symbol", bytes);
        tail.push("segoe-ui-symbol".to_owned());
    }
    regular.extend(tail.iter().cloned());
    // Without the heavier cut, titles use the regular one.
    strong.extend(regular.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, regular);
    fonts.families.insert(semibold(), strong);
    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.push("phosphor".to_owned());
    if tail.iter().any(|name| name == "segoe-ui-symbol") {
        mono.push("segoe-ui-symbol".to_owned());
    }
    ctx.set_fonts(fonts);
}

fn style_for(style: &mut egui::Style, p: &Palette, dark: bool) {
    style.text_styles = [
        (TextStyle::Small, font(SIZE_SMALL)),
        (TextStyle::Body, font(SIZE_BODY)),
        (TextStyle::Button, font(SIZE_BODY)),
        (TextStyle::Heading, font_strong(SIZE_HEADING)),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into();
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(S2, S2);
    s.button_padding = egui::vec2(12.0, 6.0);
    s.interact_size = egui::vec2(40.0, 30.0);
    s.window_margin = Margin::same(16);
    s.menu_margin = Margin::same(6);
    s.indent = 18.0;
    s.icon_width = 18.0;
    s.icon_width_inner = 10.0;
    s.combo_height = 320.0;

    let v = &mut style.visuals;
    *v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.outline);
    v.window_corner_radius = CornerRadius::same(RADIUS_CARD);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 45 }),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(if dark { 110 } else { 40 }),
    };
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.surface);
    v.faint_bg_color = p.sunken;
    v.code_bg_color = p.sunken;
    v.hyperlink_color = p.primary;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.danger;
    v.selection.bg_fill = p.primary_soft;
    v.selection.stroke = Stroke::new(1.0, p.primary);
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;
    v.slider_trailing_fill = true;
    v.weak_text_color = Some(p.muted);
    // Disabled controls stay readable (the default fades them to half).
    v.disabled_alpha = 0.62;

    let radius = CornerRadius::same(RADIUS);
    let w = &mut v.widgets;
    // Labels, separators, frames of groups.
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.outline);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    // Buttons, fields and combos at rest: white with an outline.
    w.inactive.bg_fill = p.surface;
    w.inactive.weak_bg_fill = p.surface;
    w.inactive.bg_stroke = Stroke::new(1.0, p.outline);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = radius;
    w.inactive.expansion = 0.0;
    w.hovered.bg_fill = p.sunken;
    w.hovered.weak_bg_fill = p.sunken;
    w.hovered.bg_stroke = Stroke::new(1.0, p.muted);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = radius;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = p.primary_soft;
    w.active.weak_bg_fill = p.primary_soft;
    w.active.bg_stroke = Stroke::new(1.0, p.primary);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = radius;
    w.active.expansion = 0.0;
    w.open.bg_fill = p.sunken;
    w.open.weak_bg_fill = p.sunken;
    w.open.bg_stroke = Stroke::new(1.0, p.primary);
    w.open.fg_stroke = Stroke::new(1.0, p.text);
    w.open.corner_radius = radius;
}

/// Called once when the window is created.
pub fn install(ctx: &egui::Context, appearance: Appearance) {
    install_fonts(ctx);
    ctx.style_mut_of(Theme::Light, |style| style_for(style, &LIGHT, false));
    ctx.style_mut_of(Theme::Dark, |style| style_for(style, &DARK, true));
    set_appearance(ctx, appearance);
    // The size of everything is the setting in Ajustes ▸ Aparência; egui's own
    // Ctrl +/- zoom would change it without the setting knowing.
    ctx.options_mut(|options| options.zoom_with_keyboard = false);
}

pub fn set_appearance(ctx: &egui::Context, appearance: Appearance) {
    ctx.set_theme(match appearance {
        Appearance::System => egui::ThemePreference::System,
        Appearance::Light => egui::ThemePreference::Light,
        Appearance::Dark => egui::ThemePreference::Dark,
    });
    sync(ctx);
}

/// Called at the start of every frame: the palette follows the theme egui is
/// drawing with (the system may have changed it).
pub fn sync(ctx: &egui::Context) {
    DARK_NOW.store(ctx.theme() == Theme::Dark, Ordering::Relaxed);
}

/// Icons of the bundled Phosphor font (MIT), as text: they go inside labels
/// and buttons like any other character.
pub mod icon {
    pub const DOCUMENTS: &str = "\u{e710}";
    pub const DOCUMENT: &str = "\u{e23a}";
    pub const TODAY: &str = "\u{e712}";
    pub const CIRCLE: &str = "\u{e18a}";
    pub const TASKS: &str = "\u{e184}";
    pub const ATTENDANCES: &str = "\u{e4aa}";
    pub const TEMPLATES: &str = "\u{e6d6}";
    pub const SETTINGS: &str = "\u{e434}";
    pub const HELP: &str = "\u{e3e8}";
    pub const SEARCH: &str = "\u{e30c}";
    pub const PLUS: &str = "\u{e3d4}";
    pub const SYNC: &str = "\u{e094}";
    pub const MORE: &str = "\u{e1fe}";
    pub const MAIL: &str = "\u{e218}";
    pub const CLIP: &str = "\u{e39a}";
    pub const DOWN: &str = "\u{e136}";
    pub const UP: &str = "\u{e13c}";
    pub const RIGHT: &str = "\u{e13a}";
    pub const LEFT: &str = "\u{e138}";
    pub const CLOSE: &str = "\u{e4f6}";
    pub const CHECK: &str = "\u{e182}";
    pub const CHECK_SQUARE: &str = "\u{e186}";
    pub const WARNING: &str = "\u{e4e0}";
    pub const ERROR: &str = "\u{e4e2}";
    pub const INFO: &str = "\u{e2ce}";
    pub const TRASH: &str = "\u{e4a6}";
    pub const EDIT: &str = "\u{e3b4}";
    pub const CALENDAR: &str = "\u{e10a}";
    pub const CLOCK: &str = "\u{e19a}";
    pub const STAR: &str = "\u{e46a}";
    pub const OPEN_OUT: &str = "\u{e5de}";
    pub const FILTER: &str = "\u{e266}";
    pub const LINK: &str = "\u{e2e2}";
    pub const JOINED: &str = "\u{e01a}";
    pub const COPY: &str = "\u{e1ca}";
    pub const FOLDER: &str = "\u{e256}";
    pub const DOWNLOAD: &str = "\u{e20c}";
    pub const UPLOAD: &str = "\u{e4c0}";
    pub const EXPORT: &str = "\u{eaf0}";
    pub const SAVE: &str = "\u{e248}";
    pub const AI: &str = "\u{e6a2}";
    pub const TEAM: &str = "\u{e4d6}";
    pub const CLOUD_OK: &str = "\u{e1b0}";
    pub const CLOUD_OFF: &str = "\u{e1b6}";
    pub const LOCK: &str = "\u{e308}";
    pub const EYE: &str = "\u{e220}";
    pub const EYE_OFF: &str = "\u{e224}";
    pub const BACK: &str = "\u{e058}";
    pub const UNDO: &str = "\u{e038}";
    pub const PDF: &str = "\u{e702}";
    pub const NOTE: &str = "\u{e34c}";
    pub const CHECKLIST: &str = "\u{eadc}";
    pub const BELL: &str = "\u{e0ce}";
    pub const FLAG: &str = "\u{e244}";
    pub const MOON: &str = "\u{e330}";
    pub const SUN: &str = "\u{e472}";
    pub const COMPUTER: &str = "\u{e560}";
    pub const DATABASE: &str = "\u{e1de}";
    pub const ARCHIVE: &str = "\u{e00c}";
    pub const BOOK: &str = "\u{e0e6}";
    pub const TAG: &str = "\u{e478}";
    pub const CHAIN: &str = "\u{e67c}";
    pub const FORWARD: &str = "\u{e06c}";
    pub const BROWSER: &str = "\u{e198}";
}
