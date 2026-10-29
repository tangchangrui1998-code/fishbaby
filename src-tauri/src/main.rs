#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod epub;
mod publication;
mod text;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Book {
    id: String,
    title: String,
    path: String,
    offset: usize,
    length: usize,
    fingerprint: String,
    last_read: u64,
    #[serde(default)]
    anchor: String,
    #[serde(default)]
    available: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Theme {
    #[default]
    Light,
    Dark,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum WheelMode {
    #[default]
    Mouse,
    Trackpad,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Settings {
    theme: Theme,
    font_size: f64,
    line_height: f64,
    color: String,
    background: f64,
    single_line: bool,
    wheel_mode: WheelMode,
    width: f64,
    height: f64,
    x: Option<f64>,
    y: Option<f64>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Light,
            font_size: 18.,
            line_height: 1.8,
            color: "#26332e".into(),
            background: 0.92,
            single_line: false,
            wheel_mode: WheelMode::Mouse,
            width: 420.,
            height: 160.,
            x: None,
            y: None,
        }
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Library {
    books: Vec<Book>,
    active_id: Option<String>,
    settings: Settings,
}
struct Store {
    library: Mutex<Library>,
    file: PathBuf,
    mode: Mutex<ReaderSession>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn persist(store: &Store, data: &Library) -> Result<(), String> {
    let tmp = store.file.with_extension("tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(tmp, &store.file).map_err(|e| e.to_string())
}
fn changed(app: &tauri::AppHandle) {
    let _ = app.emit("library-changed", ());
}
#[tauri::command]
fn library(app: tauri::AppHandle) -> Library {
    {
        let mut data = app.state::<Store>().library.lock().unwrap().clone();
        for b in &mut data.books {
            b.available = PathBuf::from(&b.path).is_file();
        }
        data
    }
}
fn text_at(path: &str) -> Result<String, String> {
    publication::read(path).map(|book| book.text)
}
#[tauri::command]
fn import_books(app: tauri::AppHandle, paths: Vec<String>) -> Result<Vec<String>, String> {
    let store = app.state::<Store>();
    let mut data = store.library.lock().unwrap();
    let mut errors = vec![];
    for path in paths {
        match fs::canonicalize(&path)
            .map_err(|e| e.to_string())
            .and_then(|p| {
                let p = p.to_string_lossy().to_string();
                publication::read(&p).map(|book| (p, book))
            }) {
            Ok((path, content)) => {
                if data.books.iter().any(|b| b.path == path) {
                    continue;
                }
                let id = format!("{:x}", Sha256::digest(path.as_bytes()));
                let title = content.title.unwrap_or_else(|| {
                    PathBuf::from(&path)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
                let text = content.text;
                data.books.push(Book {
                    id,
                    title,
                    path,
                    offset: 0,
                    length: text.encode_utf16().count(),
                    fingerprint: format!("{:x}", Sha256::digest(text.as_bytes())),
                    last_read: 0,
                    anchor: String::new(),
                    available: true,
                });
            }
            Err(e) => errors.push(format!(
                "{}：{e}",
                PathBuf::from(path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            )),
        }
    }
    persist(&store, &data)?;
    drop(data);
    changed(&app);
    Ok(errors)
}
#[tauri::command]
fn read_book(app: tauri::AppHandle, id: String) -> Result<(Book, String, bool), String> {
    let store = app.state::<Store>();
    let mut data = store.library.lock().unwrap();
    let b = data
        .books
        .iter_mut()
        .find(|b| b.id == id)
        .ok_or("书籍不存在")?;
    let text = text_at(&b.path)?;
    let hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    let modified = hash != b.fingerprint;
    let length = text.encode_utf16().count();
    if modified {
        b.offset = if !b.anchor.is_empty() {
            text.find(&b.anchor)
                .map(|n| text[..n].encode_utf16().count())
        } else {
            None
        }
        .unwrap_or_else(|| ((b.offset as f64 / b.length.max(1) as f64) * length as f64) as usize);
    }
    b.offset = b.offset.min(length.saturating_sub(1));
    b.length = length;
    b.fingerprint = hash;
    b.last_read = now();
    let book = b.clone();
    data.active_id = Some(id);
    persist(&store, &data)?;
    drop(data);
    changed(&app);
    let _ = app.emit("book-changed", &book.id);
    // Selecting a book opens a visible preview. Hover mode requires the reader button.
    set_mode(app.clone(), "ready".into())?;
    Ok((book, text, modified))
}
#[tauri::command]
fn current_text(app: tauri::AppHandle) -> Result<(Book, String), String> {
    let store = app.state::<Store>();
    let data = store.library.lock().unwrap();
    let b = data
        .books
        .iter()
        .find(|b| Some(&b.id) == data.active_id.as_ref())
        .ok_or("请先在书架中选择一本书")?
        .clone();
    let text = text_at(&b.path)?;
    Ok((b, text))
}
#[tauri::command]
fn save_progress(
    app: tauri::AppHandle,
    id: String,
    offset: usize,
    anchor: String,
) -> Result<(), String> {
    let store = app.state::<Store>();
    let mut data = store.library.lock().unwrap();
    if let Some(b) = data.books.iter_mut().find(|b| b.id == id) {
        b.offset = offset.min(b.length.saturating_sub(1));
        b.anchor = anchor.chars().take(80).collect();
        b.last_read = now();
    }
    persist(&store, &data)?;
    drop(data);
    changed(&app);
    Ok(())
}
#[tauri::command]
fn remove_book(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let store = app.state::<Store>();
    let mut data = store.library.lock().unwrap();
    data.books.retain(|b| b.id != id);
    if data.active_id.as_ref() == Some(&id) {
        data.active_id = None;
        *store.mode.lock().unwrap() = ReaderSession::new("paused");
        let _ = app.emit("book-changed", "");
    }
    persist(&store, &data)?;
    drop(data);
    changed(&app);
    Ok(())
}
// Style changes preserve live window dimensions; only size inputs request resize.
fn preserve_reader_size(settings: &mut Settings, resize: bool, live: Option<(f64, f64)>) {
    if resize {
        settings.width = settings.width.clamp(220., 1400.);
        settings.height = settings.height.clamp(90., 800.);
    } else if let Some((width, height)) = live {
        settings.width = width;
        settings.height = height;
    }
}
fn reader_mounted(mode: &str, state: &str) -> bool {
    mode == "hover" || state != "hidden"
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    mut settings: Settings,
    resize: Option<bool>,
) -> Result<(), String> {
    settings.font_size = settings.font_size.clamp(12., 40.);
    settings.line_height = settings.line_height.clamp(1.2, 2.5);
    settings.background = settings.background.clamp(0., 1.);
    let resize = resize.unwrap_or(false);
    let reader = app.get_webview_window("reader");
    let live_size = reader.as_ref().and_then(|w| {
        let size = w.inner_size().ok()?;
        let scale = w.scale_factor().ok()?;
        Some((size.width as f64 / scale, size.height as f64 / scale))
    });
    preserve_reader_size(&mut settings, resize, live_size);
    let store = app.state::<Store>();
    let mut data = store.library.lock().unwrap();
    settings.x = data.settings.x;
    settings.y = data.settings.y;
    data.settings = settings.clone();
    persist(&store, &data)?;
    drop(data);
    if resize {
        if let Some(w) = reader {
            w.set_size(tauri::LogicalSize::new(settings.width, settings.height))
                .map_err(|e| e.to_string())?;
        }
    }
    changed(&app);
    Ok(())
}
#[tauri::command]
fn set_mode(app: tauri::AppHandle, mode: String) -> Result<(), String> {
    change_mode(app, mode, None)
}
#[tauri::command]
fn start_reading(app: tauri::AppHandle) -> Result<(), String> {
    change_mode(app, "hover".into(), Some(true))
}
#[tauri::command]
fn reader_pointer(app: tauri::AppHandle, inside: bool) {
    app.state::<Store>().mode.lock().unwrap().pointer_inside = Some(inside);
}
fn change_mode(
    app: tauri::AppHandle,
    mode: String,
    pointer_inside: Option<bool>,
) -> Result<(), String> {
    if !["paused", "ready", "hover", "setup"].contains(&mode.as_str()) {
        return Err("无效状态".into());
    }
    let store = app.state::<Store>();
    if mode != "paused" && store.library.lock().unwrap().active_id.is_none() {
        return Err("请先选择一本书".into());
    }
    if mode == "ready" || mode == "hover" {
        if let Some(shelf) = app.get_webview_window("main") {
            if !shelf.is_minimized().map_err(|e| e.to_string())? {
                shelf.minimize().map_err(|e| e.to_string())?;
            }
        }
    }
    let mut session = ReaderSession::new(&mode);
    session.pointer_inside = pointer_inside;
    *store.mode.lock().unwrap() = session;
    Ok(())
}
#[tauri::command]
fn reset_position(app: tauri::AppHandle) -> Result<(), String> {
    let w = app.get_webview_window("reader").ok_or("阅读窗口不存在")?;
    w.center().map_err(|e| e.to_string())
}
fn open_shelf(app: &tauri::AppHandle) {
    *app.state::<Store>().mode.lock().unwrap() = ReaderSession::new("paused");
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}
#[tauri::command]
fn return_to_shelf(app: tauri::AppHandle) {
    open_shelf(&app);
}

// macOS cursor positions use the primary display scale; window bounds use
// the scale of the display hosting that window. Compare in logical coordinates.
fn cursor_in_reader(
    cursor: (f64, f64),
    cursor_scale: f64,
    origin: (f64, f64),
    size: (f64, f64),
    window_scale: f64,
) -> bool {
    let p = (cursor.0 / cursor_scale, cursor.1 / cursor_scale);
    let o = (origin.0 / window_scale, origin.1 / window_scale);
    let s = (size.0 / window_scale, size.1 / window_scale);
    p.0 >= o.0 && p.1 >= o.1 && p.0 < o.0 + s.0 && p.1 < o.1 + s.1
}

// Preview remains visible. Only explicit Start Reading enables hover hiding.
struct ReaderSession {
    name: String,
    entered_at: Option<Instant>,
    pointer_inside: Option<bool>,
}
impl ReaderSession {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            pointer_inside: None,
            // Starting from the reader button should not flash hidden for 150 ms.
            entered_at: (name == "hover").then(|| Instant::now() - Duration::from_millis(150)),
        }
    }
    fn reopen_shelf_on_activation(&self) -> bool {
        self.name == "paused"
    }
    fn update(&mut self, global_inside: bool, now: Instant) -> &'static str {
        let inside = self.pointer_inside.unwrap_or(global_inside);
        match self.name.as_str() {
            "ready" => "ready",
            "setup" => "setup",
            "hover" => {
                if !inside {
                    self.pointer_inside = None;
                    self.entered_at = None;
                    return "hidden";
                }
                let entered = self.entered_at.get_or_insert(now);
                if now.saturating_duration_since(*entered) >= Duration::from_millis(150) {
                    "reading"
                } else {
                    "hidden"
                }
            }
            _ => "hidden",
        }
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            library,
            import_books,
            read_book,
            current_text,
            save_progress,
            remove_book,
            save_settings,
            set_mode,
            start_reading,
            reader_pointer,
            reset_position,
            return_to_shelf
        ])
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            fs::create_dir_all(&dir)?;
            let file = dir.join("library.json");
            let data = if file.exists() {
                serde_json::from_slice::<Library>(&fs::read(&file)?)?
            } else {
                Library::default()
            };
            let s = data.settings.clone();
            app.manage(Store {
                library: Mutex::new(data),
                file,
                mode: Mutex::new(ReaderSession::new("paused")),
            });
            let w = app.get_webview_window("reader").unwrap();
            w.set_size(tauri::LogicalSize::new(s.width, s.height))?;
            if let (Some(x), Some(y)) = (s.x, s.y) {
                w.set_position(tauri::LogicalPosition::new(x, y))?;
                let pos = w.outer_position()?;
                let size = w.outer_size()?;
                let visible = w.available_monitors()?.iter().any(|m| {
                    let p = m.position();
                    let z = m.size();
                    pos.x + size.width as i32 > p.x
                        && pos.y + size.height as i32 > p.y
                        && pos.x < p.x + z.width as i32
                        && pos.y < p.y + z.height as i32
                });
                if !visible {
                    w.center()?;
                }
            } else {
                w.center()?;
            }
            w.set_ignore_cursor_events(true)?;
            use tauri::menu::{Menu, MenuItem};
            let shelf = MenuItem::with_id(app, "shelf", "打开书架", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "暂停 / 继续阅读", true, None::<&str>)?;
            let setup = MenuItem::with_id(app, "setup", "调整阅读区域", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出隐阅", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&shelf, &pause, &setup, &quit])?;
            let mut pixels = vec![0u8; 32 * 32 * 4];
            for y in 5..27 {
                for x in 6..26 {
                    if x < 9 || x > 22 || y == 5 || y == 26 || x == 15 {
                        let i = (y * 32 + x) * 4;
                        pixels[i + 3] = 255;
                    }
                }
            }
            tauri::tray::TrayIconBuilder::new()
                .icon(tauri::image::Image::new_owned(pixels, 32, 32))
                .icon_as_template(true)
                .tooltip("隐阅")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "shelf" => open_shelf(app),
                    "pause" => {
                        let mode = app.state::<Store>().mode.lock().unwrap().name.clone();
                        let _ = set_mode(
                            app.clone(),
                            if mode == "paused" { "hover" } else { "paused" }.into(),
                        );
                    }
                    "setup" => {
                        let _ = set_mode(app.clone(), "setup".into());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            app.global_shortcut()
                .on_shortcut("CommandOrControl+Shift+H", |app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        let mode = app.state::<Store>().mode.lock().unwrap().name.clone();
                        let _ = set_mode(
                            app.clone(),
                            if mode == "paused" { "hover" } else { "paused" }.into(),
                        );
                    }
                })?;
            app.global_shortcut()
                .on_shortcut("CommandOrControl+Shift+O", |app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        open_shelf(app);
                    }
                })?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut previous = (String::new(), false);
                loop {
                    std::thread::sleep(Duration::from_millis(30));
                    let store = handle.state::<Store>();
                    let Some(w) = handle.get_webview_window("reader") else {
                        break;
                    };
                    let window_scale = w.scale_factor().unwrap_or(1.);
                    #[cfg(target_os = "macos")]
                    let cursor_scale = handle
                        .primary_monitor()
                        .ok()
                        .flatten()
                        .map(|m| m.scale_factor())
                        .unwrap_or(window_scale);
                    #[cfg(not(target_os = "macos"))]
                    let cursor_scale = window_scale;
                    let inside = if let (Ok(p), Ok(o), Ok(s)) =
                        (handle.cursor_position(), w.outer_position(), w.outer_size())
                    {
                        cursor_in_reader(
                            (p.x, p.y),
                            cursor_scale,
                            (o.x as f64, o.y as f64),
                            (s.width as f64, s.height as f64),
                            window_scale,
                        )
                    } else {
                        false
                    };
                    let (state, mounted) = {
                        let mut session = store.mode.lock().unwrap();
                        let state = session.update(inside, Instant::now());
                        (state, reader_mounted(&session.name, state))
                    };
                    if previous != (state.into(), mounted) {
                        let _ = w.set_focusable(state == "setup" || state == "ready");
                        let _ = w.set_ignore_cursor_events(state == "hidden");
                        // Hover hiding changes WebView visibility and hit testing only.
                        // Hiding the active NSWindow can restore the minimized main window.
                        if mounted {
                            if !w.is_visible().unwrap_or(false) {
                                let _ = w.show();
                            }
                        } else {
                            let _ = w.hide();
                        }
                        let _ = w.emit("reader-state", state);
                        previous = (state.into(), mounted);
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|w, event| {
            if w.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w.hide();
                }
            }
            if w.label() == "reader" {
                if let Some(store) = w.try_state::<Store>() {
                    let scale = w.scale_factor().unwrap_or(1.);
                    match event {
                        tauri::WindowEvent::Moved(p) => {
                            let mut data = store.library.lock().unwrap();
                            data.settings.x = Some(p.x as f64 / scale);
                            data.settings.y = Some(p.y as f64 / scale);
                            let _ = persist(&store, &data);
                        }
                        tauri::WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                            let mut data = store.library.lock().unwrap();
                            data.settings.width = size.width as f64 / scale;
                            data.settings.height = size.height as f64 / scale;
                            let _ = persist(&store, &data);
                            drop(data);
                            changed(w.app_handle());
                        }
                        _ => {}
                    }
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("启动隐阅失败")
        .run(|_app, _event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                // Reactivating a reader window must not act like Return to Bookshelf.
                // Drop the session lock before window operations dispatch native events.
                let reopen = _app
                    .state::<Store>()
                    .mode
                    .lock()
                    .unwrap()
                    .reopen_shelf_on_activation();
                if reopen {
                    open_shelf(_app);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_book_stays_visible_until_explicit_start() {
        let start = Instant::now();
        let mut session = ReaderSession::new("ready");
        for inside in [false, true, false, true, false] {
            assert_eq!(
                session.update(inside, start + Duration::from_secs(60)),
                "ready"
            );
        }
        session = ReaderSession::new("hover");
        let start = Instant::now();
        assert_eq!(session.update(true, start), "reading");
        assert_eq!(
            session.update(false, start + Duration::from_secs(1)),
            "hidden"
        );
        assert_eq!(
            session.update(true, start + Duration::from_secs(2)),
            "hidden"
        );
        assert_eq!(
            session.update(true, start + Duration::from_millis(2150)),
            "reading"
        );
        assert_eq!(
            session.update(false, start + Duration::from_secs(3)),
            "hidden"
        );
    }
    #[test]
    fn returning_to_shelf_hides_reader_and_reopening_requires_start_again() {
        let now = Instant::now();
        assert_eq!(ReaderSession::new("paused").update(true, now), "hidden");
        assert_eq!(ReaderSession::new("setup").update(false, now), "setup");
        let mut reopened = ReaderSession::new("ready");
        assert_eq!(reopened.update(false, now), "ready");
        assert_eq!(reopened.update(true, now), "ready");
        assert_eq!(reopened.update(false, now), "ready");
    }
    #[test]
    fn activation_never_returns_an_active_reader_to_bookshelf() {
        for mode in ["ready", "setup", "hover"] {
            let mut session = ReaderSession::new(mode);
            let now = Instant::now();
            assert!(!session.reopen_shelf_on_activation());
            if mode == "hover" {
                assert_eq!(session.update(true, now), "reading");
                assert!(!session.reopen_shelf_on_activation());
                assert_eq!(session.update(false, now), "hidden");
                assert!(!session.reopen_shelf_on_activation());
            }
        }
        assert!(ReaderSession::new("paused").reopen_shelf_on_activation());
    }
    #[test]
    fn pointer_hit_test_handles_mixed_display_scales() {
        for (cursor_scale, window_scale) in [(1., 1.), (2., 1.), (1., 2.), (2., 2.)] {
            let origin = (300. * window_scale, 200. * window_scale);
            let size = (420. * window_scale, 160. * window_scale);
            assert!(cursor_in_reader(
                (400. * cursor_scale, 250. * cursor_scale),
                cursor_scale,
                origin,
                size,
                window_scale
            ));
            assert!(!cursor_in_reader(
                (725. * cursor_scale, 250. * cursor_scale),
                cursor_scale,
                origin,
                size,
                window_scale
            ));
            assert!(!cursor_in_reader(
                (400. * cursor_scale, 365. * cursor_scale),
                cursor_scale,
                origin,
                size,
                window_scale
            ));
        }
    }
    #[test]
    fn start_click_uses_reader_pointer_until_exit_then_global_position_can_wake_it() {
        let mut session = ReaderSession::new("hover");
        let now = Instant::now();
        session.pointer_inside = Some(true);
        assert_eq!(session.update(false, now), "reading");
        session.pointer_inside = Some(false);
        assert_eq!(session.update(false, now), "hidden");
        assert_eq!(session.update(true, now), "hidden");
        assert_eq!(
            session.update(true, now + Duration::from_millis(150)),
            "reading"
        );
    }
    #[test]
    fn theme_change_keeps_manual_size_and_explicit_resize_still_applies() {
        let mut settings = Settings::default();
        settings.theme = Theme::Dark;
        preserve_reader_size(&mut settings, false, Some((736., 284.)));
        assert_eq!((settings.width, settings.height), (736., 284.));
        settings.theme = Theme::Light;
        preserve_reader_size(&mut settings, false, Some((736., 284.)));
        let saved: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!((saved.width, saved.height), (736., 284.));
        settings.width = 600.;
        settings.height = 210.;
        preserve_reader_size(&mut settings, true, Some((736., 284.)));
        assert_eq!((settings.width, settings.height), (600., 210.));
    }
    #[test]
    fn first_pointer_exit_keeps_native_reader_mounted_and_session_in_hover() {
        let mut session = ReaderSession::new("hover");
        session.pointer_inside = Some(true);
        let now = Instant::now();
        let visible = session.update(false, now);
        assert_eq!(visible, "reading");
        assert!(reader_mounted(&session.name, visible));
        session.pointer_inside = Some(false);
        let hidden = session.update(false, now);
        assert_eq!(hidden, "hidden");
        assert!(reader_mounted(&session.name, hidden));
        assert!(!session.reopen_shelf_on_activation());
        assert_eq!(session.name, "hover");
        assert_eq!(session.update(true, now), "hidden");
        assert_eq!(
            session.update(true, now + Duration::from_millis(150)),
            "reading"
        );
        assert!(!reader_mounted("paused", "hidden"));
    }
    #[test]
    fn older_settings_default_to_light_and_themes_roundtrip() {
        let mut settings: Settings = serde_json::from_str(r##"{"fontSize":20}"##).unwrap();
        assert!(matches!(settings.theme, Theme::Light));
        assert!(matches!(settings.wheel_mode, WheelMode::Mouse));
        settings.theme = Theme::Dark;
        settings.wheel_mode = WheelMode::Trackpad;
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(matches!(restored.theme, Theme::Dark));
        assert!(matches!(restored.wheel_mode, WheelMode::Trackpad));
    }
    #[test]
    fn txt_import_normalizes_text_and_accepts_gbk() {
        let path = std::env::temp_dir().join(format!("fishbaby-{}.txt", std::process::id()));
        fs::write(&path, "\u{feff}甲😀\r\n乙").unwrap();
        assert_eq!(text_at(path.to_str().unwrap()).unwrap(), "甲😀\n乙");
        fs::write(&path, [0xd6, 0xd0, 0xce, 0xc4]).unwrap();
        assert_eq!(text_at(path.to_str().unwrap()).unwrap(), "中文");
        fs::write(&path, [0x81]).unwrap();
        assert!(text_at(path.to_str().unwrap())
            .unwrap_err()
            .contains("无法解码"));
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn persisted_books_keep_independent_positions() {
        let raw = r##"{"books":[{"id":"a","title":"甲","path":"a.txt","offset":42,"length":100,"fingerprint":"a","lastRead":1},{"id":"b","title":"乙","path":"b.txt","offset":15,"length":200,"fingerprint":"b","lastRead":2}],"activeId":"b"}"##;
        let data: Library = serde_json::from_str(raw).unwrap();
        let roundtrip: Library =
            serde_json::from_slice(&serde_json::to_vec(&data).unwrap()).unwrap();
        assert_eq!(roundtrip.active_id.as_deref(), Some("b"));
        assert_eq!(
            roundtrip.books.iter().map(|b| b.offset).collect::<Vec<_>>(),
            vec![42, 15]
        );
    }
}
