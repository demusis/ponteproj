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
    file_place(title, suggested, filters, true)
}

/// The same dialog for choosing where a file lives when choosing it writes
/// nothing: a file that already exists there is taken as it is, so Windows
/// does not ask whether to replace it (the question frightens: nothing is
/// replaced).
pub fn choose_file_place(
    title: &str,
    suggested: &str,
    filters: &[(&str, &str)],
) -> Option<PathBuf> {
    file_place(title, suggested, filters, false)
}

fn file_place(
    title: &str,
    suggested: &str,
    filters: &[(&str, &str)],
    ask_before_replacing: bool,
) -> Option<PathBuf> {
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
        Flags: if ask_before_replacing {
            OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR
        } else {
            OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR
        },
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
    open_with_default_app(Path::new(url)).map_err(|_| {
        anyhow::anyhow!(
            "O Windows não conseguiu abrir o navegador. Copie o endereço e abra-o manualmente."
        )
    })
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
        .or_else(|| {
            crate::paths::data_dir()
                .ok()
                .map(|d| d.join(format!("{host}.json")))
        });
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

// ---------------------------------------------------------------------------
// Notification-area icon with a balloon (shown by Windows 10/11 as a toast).
//
// A small hidden window owns the icon. It must be created on the thread that
// runs the interface message loop (the window thread), which is where the
// application calls these functions; winit's loop dispatches its messages.
// Nothing here touches the library, the registry or the disk.
// ---------------------------------------------------------------------------

mod tray {
    use super::wide;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
    use std::sync::Mutex;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_NOSOUND,
        NIM_ADD, NIM_DELETE, NIM_MODIFY, NIN_BALLOONUSERCLICK, NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, LoadIconW, RegisterClassW,
        RegisterWindowMessageW, UnregisterClassW, HICON, IDI_APPLICATION, WINDOW_EX_STYLE,
        WINDOW_STYLE, WM_LBUTTONUP, WM_USER, WNDCLASSW,
    };

    const CLASS_NAME: &str = "PonteProjLembretes";
    const TRAY_ID: u32 = 1;
    const CALLBACK_MESSAGE: u32 = WM_USER + 1;

    /// The hidden window (0 = none), whether the icon is in the notification
    /// area, whether the person clicked, and the id of the "TaskbarCreated"
    /// broadcast (Explorer restarted: the icon has to be added again).
    static WINDOW: AtomicIsize = AtomicIsize::new(0);
    static ICON_ADDED: AtomicBool = AtomicBool::new(false);
    static CLICKED: AtomicBool = AtomicBool::new(false);
    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
    /// Wakes the interface after a click, so a minimised window notices it.
    #[allow(clippy::type_complexity)]
    static WAKE: Mutex<Option<Box<dyn Fn() + Send + Sync>>> = Mutex::new(None);

    pub fn set_wake(wake: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut slot) = WAKE.lock() {
            *slot = Some(Box::new(wake));
        }
    }

    pub fn take_click() -> bool {
        CLICKED.swap(false, Ordering::SeqCst)
    }

    fn hwnd_of(value: isize) -> HWND {
        HWND(value as *mut c_void)
    }

    fn copy_text(target: &mut [u16], text: &str) {
        // Keep room for the final NUL; the buffers start zeroed.
        let limit = target.len() - 1;
        for (slot, unit) in target.iter_mut().zip(text.encode_utf16().take(limit)) {
            *slot = unit;
        }
    }

    fn base_data(hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            ..Default::default()
        }
    }

    fn module() -> Option<HINSTANCE> {
        // SAFETY: asks for the handle of the running executable; no ownership.
        unsafe { GetModuleHandleW(PCWSTR::null()) }
            .ok()
            .map(|handle| HINSTANCE(handle.0))
    }

    /// The icon of the executable (resource 1), or the generic application
    /// icon. Shared icons: they are never destroyed.
    fn app_icon() -> HICON {
        // SAFETY: loading a shared icon from this module or from the system;
        // MAKEINTRESOURCE(1) is the identifier the resource script gives it.
        unsafe {
            LoadIconW(module(), PCWSTR(std::ptr::without_provenance::<u16>(1)))
                .or_else(|_| LoadIconW(None, IDI_APPLICATION))
                .unwrap_or_default()
        }
    }

    fn add_icon(hwnd: HWND) -> bool {
        let mut data = base_data(hwnd);
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = CALLBACK_MESSAGE;
        data.hIcon = app_icon();
        copy_text(&mut data.szTip, "Ponte");
        // SAFETY: `data` is fully initialised and outlives the call.
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool();
        ICON_ADDED.store(added, Ordering::SeqCst);
        added
    }

    fn delete_icon(hwnd: HWND) {
        if ICON_ADDED.swap(false, Ordering::SeqCst) {
            let data = base_data(hwnd);
            // SAFETY: removes the icon this window added; failure is harmless.
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
        }
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == CALLBACK_MESSAGE {
            // Version-0 notifications: `lparam` is the event.
            let event = (lparam.0 as u32) & 0xFFFF;
            if wparam.0 as u32 == TRAY_ID
                && (event == NIN_BALLOONUSERCLICK || event == WM_LBUTTONUP)
            {
                CLICKED.store(true, Ordering::SeqCst);
                delete_icon(hwnd);
                if let Ok(slot) = WAKE.lock() {
                    if let Some(wake) = slot.as_ref() {
                        wake();
                    }
                }
            }
            return LRESULT(0);
        }
        let created = TASKBAR_CREATED.load(Ordering::SeqCst);
        if created != 0 && message == created && ICON_ADDED.load(Ordering::SeqCst) {
            // Explorer restarted: its notification area is empty again.
            add_icon(hwnd);
            return LRESULT(0);
        }
        // SAFETY: default handling of a message this window does not use.
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    /// The hidden window, created on first use. A top-level window that is
    /// never shown (message-only windows do not get the TaskbarCreated
    /// broadcast).
    fn window() -> Option<HWND> {
        let existing = WINDOW.load(Ordering::SeqCst);
        if existing != 0 {
            return Some(hwnd_of(existing));
        }
        let class = wide(CLASS_NAME);
        let module = module();
        // SAFETY: the class name outlives the calls; the window procedure is a
        // plain function; the window is destroyed in `shutdown`.
        unsafe {
            let description = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module.unwrap_or_default(),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            // Registering twice fails harmlessly (the class already exists).
            RegisterClassW(&description);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR(class.as_ptr()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                None,
                None,
                module,
                None,
            )
            .ok()?;
            let broadcast = wide("TaskbarCreated");
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(PCWSTR(broadcast.as_ptr())),
                Ordering::SeqCst,
            );
            WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
            Some(hwnd)
        }
    }

    pub fn notify(title: &str, text: &str) -> bool {
        let Some(hwnd) = window() else {
            return false;
        };
        if !ICON_ADDED.load(Ordering::SeqCst) && !add_icon(hwnd) {
            return false;
        }
        // An empty balloon first: Windows ignores a balloon identical to the
        // one that is still on screen.
        let mut clear = base_data(hwnd);
        clear.uFlags = NIF_INFO;
        // SAFETY: `clear` is initialised and outlives the call.
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &clear) };
        let mut data = base_data(hwnd);
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_INFO | NIIF_NOSOUND;
        copy_text(&mut data.szInfoTitle, title);
        copy_text(&mut data.szInfo, text);
        // SAFETY: `data` is initialised and outlives the call.
        let shown = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool();
        if !shown {
            // The shell lost the icon (it restarted): add it once more and retry.
            ICON_ADDED.store(false, Ordering::SeqCst);
            if add_icon(hwnd) {
                // SAFETY: as above.
                return unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) }.as_bool();
            }
        }
        shown
    }

    pub fn shutdown() {
        let existing = WINDOW.swap(0, Ordering::SeqCst);
        if existing == 0 {
            return;
        }
        let hwnd = hwnd_of(existing);
        delete_icon(hwnd);
        let class = wide(CLASS_NAME);
        // SAFETY: destroys the window created by `window` (on the same thread)
        // and unregisters its class; failures leave nothing to clean up.
        unsafe {
            let _ = DestroyWindow(hwnd);
            let _ = UnregisterClassW(PCWSTR(class.as_ptr()), module());
        }
    }
}

/// Shows a balloon (a toast on Windows 10/11) from a notification-area icon
/// that is added if needed. Call from the window thread. Returns `false`
/// when Windows refused the icon or the window could not be created; whether
/// Windows then displays it depends on the person's notification settings.
pub fn show_notification(title: &str, text: &str) -> bool {
    tray::notify(title, text)
}

/// `true` once after the person clicked the notification or its icon.
pub fn take_notification_click() -> bool {
    tray::take_click()
}

/// Function called (from the window thread) when the notification is clicked,
/// to wake the interface, e.g. `|| ctx.request_repaint()`.
pub fn on_notification_click(wake: impl Fn() + Send + Sync + 'static) {
    tray::set_wake(wake);
}

/// Removes the notification-area icon and the hidden window. Safe to call
/// more than once, or when nothing was ever shown.
pub fn shutdown_notifications() {
    tray::shutdown();
}
