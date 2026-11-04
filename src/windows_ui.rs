//! Small Win32 helpers used by the interface: the standard "open file" dialog
//! and a read-only check of the browser bridge installation. Nothing here
//! writes to the registry or to disk.

use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Standard Windows dialog to choose one existing file.
/// `filters` are `(description, pattern)` pairs such as `("Planilhas Excel", "*.xlsx")`.
pub fn pick_file(title: &str, filters: &[(&str, &str)]) -> Option<PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };
    let mut filter = String::new();
    for (description, pattern) in filters {
        filter.push_str(&format!("{description} ({pattern})\0{pattern}\0"));
    }
    filter.push('\0');
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let title = wide(title);
    let mut buffer = vec![0u16; 32 * 1024];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    // SAFETY: every pointer in `dialog` refers to a buffer that outlives the call.
    let chosen = unsafe { GetOpenFileNameW(&mut dialog) }.as_bool();
    if !chosen {
        return None;
    }
    let length = buffer.iter().position(|c| *c == 0).unwrap_or(0);
    let path = PathBuf::from(String::from_utf16_lossy(&buffer[..length]));
    path.is_file().then_some(path)
}

/// Standard Windows dialog to choose where to save a file. `suggested` is
/// the initial file name; the person may change it. Asks before overwriting.
pub fn save_file(title: &str, suggested: &str, filters: &[(&str, &str)]) -> Option<PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{
        GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
        OPENFILENAMEW,
    };
    let mut filter = String::new();
    for (description, pattern) in filters {
        filter.push_str(&format!("{description} ({pattern})\0{pattern}\0"));
    }
    filter.push('\0');
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let title = wide(title);
    let mut buffer = vec![0u16; 32 * 1024];
    let initial: Vec<u16> = suggested.encode_utf16().take(buffer.len() - 1).collect();
    buffer[..initial.len()].copy_from_slice(&initial);
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(buffer.as_mut_ptr()),
        nMaxFile: buffer.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    // SAFETY: every pointer in `dialog` refers to a buffer that outlives the call.
    let chosen = unsafe { GetSaveFileNameW(&mut dialog) }.as_bool();
    if !chosen {
        return None;
    }
    let length = buffer.iter().position(|c| *c == 0).unwrap_or(0);
    let path = PathBuf::from(String::from_utf16_lossy(&buffer[..length]));
    (!path.as_os_str().is_empty()).then_some(path)
}

/// Opens a file with the program Windows associates to its type (the "open"
/// verb of the shell), without running the file itself.
pub fn open_with_default_app(path: &Path) -> anyhow::Result<()> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let verb = wide("open");
    let target = wide(&path.to_string_lossy());
    // SAFETY: the wide strings outlive the call; a null window and parameters are allowed.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // Per the Win32 contract, values above 32 mean success.
    anyhow::ensure!(
        result.0 as usize > 32,
        "o Windows não tem um programa associado a este tipo de arquivo, ou recusou a abertura. Use “Salvar como…” e abra a cópia manualmente."
    );
    Ok(())
}

/// Opens an HTTPS address in the default browser (sign-in pages).
pub fn open_url(url: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        url.starts_with("https://") && !url.chars().any(|c| c.is_control() || c.is_whitespace()),
        "Endereço inválido para abrir no navegador."
    );
    open_with_default_app(Path::new(url))
        .map_err(|_| anyhow::anyhow!("O Windows não conseguiu abrir o navegador. Copie o endereço e abra-o manualmente."))
}

/// Default string value of a key under HKEY_CURRENT_USER, if present.
fn user_registry_default(subkey: &str) -> Option<String> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let subkey = wide(subkey);
    let mut size: u32 = 0;
    // SAFETY: a null data pointer only queries the required size.
    let probe = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if probe.is_err() || size == 0 {
        return None;
    }
    let mut data = vec![0u16; (size as usize).div_ceil(2) + 1];
    // SAFETY: `data` holds at least `size` bytes, as reported by the probe.
    let read = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(data.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if read.is_err() {
        return None;
    }
    let length = data.iter().position(|c| *c == 0).unwrap_or(data.len());
    Some(String::from_utf16_lossy(&data[..length]))
}

/// What the installer left behind for the browser bridge, read-only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstallStatus {
    pub chrome_registered: bool,
    pub edge_registered: bool,
    /// Manifest the browsers are pointed to, when registered.
    pub manifest: Option<PathBuf>,
    /// The manifest exists and names an executable that exists.
    pub manifest_valid: bool,
    /// Folder of the extension to load in the browser, when found.
    pub extension_dir: Option<PathBuf>,
}

impl InstallStatus {
    pub fn browser_bridge_ok(&self) -> bool {
        (self.chrome_registered || self.edge_registered) && self.manifest_valid
    }
}

pub fn installation_status() -> InstallStatus {
    let host = crate::native::HOST_NAME;
    let chrome = user_registry_default(&format!(
        "Software\\Google\\Chrome\\NativeMessagingHosts\\{host}"
    ));
    let edge = user_registry_default(&format!(
        "Software\\Microsoft\\Edge\\NativeMessagingHosts\\{host}"
    ));
    let manifest = chrome
        .clone()
        .or(edge.clone())
        .map(PathBuf::from)
        .or_else(|| crate::paths::data_dir().ok().map(|d| d.join(format!("{host}.json"))));
    let manifest_valid = manifest.as_deref().is_some_and(manifest_names_existing_exe);
    let extension_dir = locate_extension_dir();
    InstallStatus {
        chrome_registered: chrome.is_some(),
        edge_registered: edge.is_some(),
        manifest,
        manifest_valid,
        extension_dir,
    }
}

fn manifest_names_existing_exe(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    value
        .get("path")
        .and_then(|p| p.as_str())
        .is_some_and(|exe| Path::new(exe).is_file())
}

fn locate_extension_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        // %LOCALAPPDATA%\Programs\PonteProj\vX.Y.Z\ponteproj.exe → ..\extension
        if let Some(root) = exe.parent().and_then(Path::parent) {
            candidates.push(root.join("extension"));
        }
    }
    if let Some(dirs) = directories::BaseDirs::new() {
        candidates.push(
            dirs.data_local_dir()
                .join("Programs")
                .join("PonteProj")
                .join("extension"),
        );
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("manifest.json").is_file())
}

/// Opens the folder in Explorer. Never fails the interface.
pub fn reveal_in_explorer(path: &Path) {
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}
