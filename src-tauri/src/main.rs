#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod diagnostics;
mod epub;
mod features;
mod publication;
#[cfg(any(target_os = "macos", windows))]
mod reader_geometry;
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
    tools: features::ReadingTools,
}
struct Store {
    library: Mutex<Library>,
    file: PathBuf,
    mode: Mutex<ReaderSession>,
    presentation: Mutex<ReaderView>,
    shortcut_edit: tauri::async_runtime::Mutex<()>,
    geometry_generation: std::sync::atomic::AtomicU64,
}
#[derive(Clone, Serialize)]
struct ReaderView {
    state: String,
    revision: u64,
}
impl Default for ReaderView {
    fn default() -> Self {
        Self {
            state: "hidden".into(),
            revision: 0,
        }
    }
}
impl ReaderView {
    fn publish(&mut self, state: &str) -> Self {
        self.state = state.into();
        self.revision += 1;
        self.clone()
    }
}
#[tauri::command]
fn reader_state(app: tauri::AppHandle) -> ReaderView {
    app.state::<Store>().presentation.lock().unwrap().clone()
}
#[tauri::command]
fn reader_report(
    window: tauri::WebviewWindow,
    revision: u64,
    state: String,
    hovered: bool,
    visible: bool,
    content_visible: bool,
) {
    if window.label() == "reader"
        && ["hidden", "reading", "ready", "setup"].contains(&state.as_str())
    {
        diagnostics::record(
            "frontend-view",
            serde_json::json!({
            "revision": revision, "state": state, "hovered": hovered, "pageVisible": visible,
            "contentVisible": content_visible,
            }),
        );
    }
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
    data.tools.bookmarks.remove(&id);
    if data.active_id.as_ref() == Some(&id) {
        data.active_id = None;
        store.mode.lock().unwrap().reset("paused", None);
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
    #[cfg(not(any(target_os = "macos", windows)))]
    let live_size = reader.as_ref().and_then(|w| {
        let size = w.inner_size().ok()?;
        let scale = w.scale_factor().ok()?;
        Some((size.width as f64 / scale, size.height as f64 / scale))
    });
    let store = app.state::<Store>();
    let mut data = store.library.lock().unwrap();
    #[cfg(any(target_os = "macos", windows))]
    let live_size = Some((data.settings.width, data.settings.height));
    preserve_reader_size(&mut settings, resize, live_size);
    settings.x = data.settings.x;
    settings.y = data.settings.y;
    data.settings = settings.clone();
    persist(&store, &data)?;
    drop(data);
    if resize {
        if let Some(w) = reader {
            #[cfg(any(target_os = "macos", windows))]
            reader_geometry::resize_reader(&w, (settings.width, settings.height))
                .map_err(|e| e.to_string())?;
            #[cfg(not(any(target_os = "macos", windows)))]
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
    app.state::<Store>()
        .mode
        .lock()
        .unwrap()
        .set_pointer(inside, Instant::now());
    diagnostics::record("pointer-event", serde_json::json!({ "inside": inside }));
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
    store.mode.lock().unwrap().reset(&mode, pointer_inside);
    Ok(())
}
#[tauri::command]
fn reset_position(app: tauri::AppHandle) -> Result<(), String> {
    let w = app.get_webview_window("reader").ok_or("阅读窗口不存在")?;
    w.center().map_err(|e| e.to_string())
}
fn open_shelf(app: &tauri::AppHandle) {
    app.state::<Store>()
        .mode
        .lock()
        .unwrap()
        .reset("paused", None);
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
    pointer_at: Option<Instant>,
    generation: u64,
}
impl ReaderSession {
    fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            generation: 0,
            pointer_inside: None,
            pointer_at: None,
            // Starting from the reader button should not flash hidden for 150 ms.
            entered_at: (name == "hover").then(|| Instant::now() - Duration::from_millis(150)),
        }
    }
    fn reopen_shelf_on_activation(&self) -> bool {
        self.name == "paused"
    }
    fn reset(&mut self, name: &str, pointer_inside: Option<bool>) {
        let generation = self.generation + 1;
        *self = Self::new(name);
        self.generation = generation;
        if let Some(inside) = pointer_inside {
            self.set_pointer(inside, Instant::now());
        }
        diagnostics::record(
            "mode",
            serde_json::json!({ "mode": name, "generation": generation }),
        );
    }
    fn set_pointer(&mut self, inside: bool, now: Instant) {
        self.pointer_inside = Some(inside);
        self.pointer_at = Some(now);
    }
    fn update_exposure(&mut self, inside: bool, uncovered: bool, now: Instant) -> &'static str {
        if self.name == "hover" && !uncovered {
            self.pointer_inside = None;
            self.entered_at = None;
            return "hidden";
        }
        self.update(inside, now)
    }
    fn update(&mut self, global_inside: bool, now: Instant) -> &'static str {
        // DOM leave can be lost during pointer capture, native dragging or WebView
        // focus changes. A positive hint only bridges the Start click; it must
        // never override the desktop cursor indefinitely. 90 ms + 30 ms polling
        // keeps a missed leave within the 150 ms hide budget.
        if self.pointer_inside == Some(true)
            && self
                .pointer_at
                .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_millis(90))
        {
            if !global_inside {
                diagnostics::record(
                    "pointer-hint-expired-outside",
                    serde_json::json!({
                        "mode": self.name,
                    }),
                );
            }
            self.pointer_inside = None;
        }
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
            features::reading_navigation,
            features::save_bookmark,
            features::remove_bookmark,
            features::app_information,
            features::open_log_directory,
            features::export_diagnostics,
            features::shortcuts::save_shortcuts,
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
            reader_state,
            reader_report,
            reset_position,
            return_to_shelf
        ])
        .setup(|app| {
            diagnostics::init(app.path().app_log_dir()?);
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
                presentation: Mutex::new(ReaderView::default()),
                shortcut_edit: tauri::async_runtime::Mutex::new(()),
                geometry_generation: std::sync::atomic::AtomicU64::new(u64::MAX),
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
            let bindings = app.state::<Store>().library.lock().unwrap().tools.shortcuts.clone();
            features::shortcuts::install(app.handle(), &bindings);
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                #[cfg(any(target_os = "macos", windows))]
                let mut geometry = reader_geometry::Protection::new();
                let mut displays = features::displays::Tracker::new();
                let mut previous: Option<(String, bool, u64)> = None;
                let mut last_snapshot = Instant::now() - Duration::from_secs(5);
                let mut last_poll = Instant::now();
                loop {
                    std::thread::sleep(Duration::from_millis(30));
                    let store = handle.state::<Store>();
                    let Some(w) = handle.get_webview_window("reader") else {
                        continue;
                    };
                    if let Err(error) = displays.update(&w, &store) {
                        diagnostics::record("display-layout-error", serde_json::json!({"error": error.to_string()}));
                    }
                    #[cfg(any(target_os = "macos", windows))]
                    match geometry.reconcile(&w, &store) {
                        Ok(true) => {},
                        Ok(false) => continue,
                        Err(error) => {
                            diagnostics::record("geometry-error", serde_json::json!({ "error": error.to_string() }));
                            continue;
                        }
                    }
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
                    let cursor = handle.cursor_position();
                    let origin = w.outer_position();
                    let size = w.outer_size();
                    let poll_gap = last_poll.elapsed().as_millis();
                    last_poll = Instant::now();
                    let inside = if let (Ok(p), Ok(o), Ok(s)) =
                        (cursor.as_ref(), origin.as_ref(), size.as_ref())
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
                    let (state, mounted, generation) = {
                        let mut session = store.mode.lock().unwrap();
                        let state = session.update_exposure(inside, true, Instant::now());
                        (
                            state,
                            reader_mounted(&session.name, state),
                            session.generation,
                        )
                    };
                    let next = (state.into(), mounted, generation);
                    if previous.as_ref() != Some(&next) || last_snapshot.elapsed() >= Duration::from_secs(5) {
                        let session = store.mode.lock().unwrap();
                        diagnostics::record("reader-snapshot", serde_json::json!({
                            "mode": session.name, "state": state, "mounted": mounted,
                            "generation": generation, "globalInside": inside,
                            "pointerHint": session.pointer_inside,
                            "pointerAgeMs": session.pointer_at.map(|at| at.elapsed().as_millis()),
                            "cursor": cursor.as_ref().ok().map(|p| (p.x, p.y)),
                            "origin": origin.as_ref().ok().map(|p| (p.x, p.y)),
                            "size": size.as_ref().ok().map(|s| (s.width, s.height)),
                            "cursorScale": cursor_scale, "windowScale": window_scale,
                            "pollGapMs": poll_gap,
                            "cursorError": cursor.as_ref().err().map(|e| e.to_string()),
                            "originError": origin.as_ref().err().map(|e| e.to_string()),
                            "sizeError": size.as_ref().err().map(|e| e.to_string()),
                        }));
                        last_snapshot = Instant::now();
                    }
                    if previous.as_ref() != Some(&next) {
                        if let Err(error) = w.set_focusable(state == "setup" || state == "ready") {
                            diagnostics::record("focusable-error", serde_json::json!({ "error": error.to_string() }));
                        }
                        if let Err(error) = w.set_ignore_cursor_events(state == "hidden") {
                            diagnostics::record("hit-test-error", serde_json::json!({ "error": error.to_string() }));
                            continue;
                        }
                        // Hover hiding changes WebView visibility and hit testing only.
                        // Hiding the active NSWindow can restore the minimized main window.
                        if mounted {
                            if !w.is_visible().unwrap_or(false) {
                                if let Err(error) = w.show() {
                                    diagnostics::record("show-error", serde_json::json!({ "error": error.to_string() }));
                                    continue;
                                }
                            }
                        } else if let Err(error) = w.hide() {
                            diagnostics::record("hide-error", serde_json::json!({ "error": error.to_string() }));
                            continue;
                        }
                        #[cfg(target_os = "macos")]
                        if matches!(state, "ready" | "setup")
                            && previous.as_ref().is_none_or(|p| p.2 != generation)
                        {
                            // Explicit preview/tool requests may focus the reader;
                            // ordinary hover transitions must never take focus.
                            let _ = w.set_focus();
                        }
                        let view = store.presentation.lock().unwrap().publish(state);
                        if let Err(error) = w.emit("reader-state", &view) {
                            diagnostics::record("emit-error", serde_json::json!({ "error": error.to_string() }));
                            continue;
                        }
                        diagnostics::record("reader-view", serde_json::json!({ "revision": view.revision, "state": view.state }));
                        previous = Some(next);
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
                #[cfg(any(target_os = "macos", windows))]
                reader_geometry::on_event(w, event);
                #[cfg(not(any(target_os = "macos", windows)))]
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
    fn late_frontend_can_hydrate_current_state_and_repeated_requests_keep_advancing() {
        let mut view = ReaderView::default();
        let ready = view.publish("ready");
        assert_eq!(ready.state, "ready");
        assert_eq!(view.clone().revision, ready.revision);
        let mut session = ReaderSession::new("ready");
        session.reset("ready", None);
        assert_eq!(session.generation, 1);
        session.reset("ready", None);
        assert_eq!(session.generation, 2);
        let second = view.publish("ready");
        assert!(second.revision > ready.revision);
    }
    #[test]
    fn covering_reader_clears_stale_pointer_hint_and_requires_fresh_hover() {
        let mut session = ReaderSession::new("hover");
        session.set_pointer(true, Instant::now());
        let now = Instant::now();
        assert_eq!(session.update_exposure(true, true, now), "reading");
        assert_eq!(session.update_exposure(true, false, now), "hidden");
        assert_eq!(session.pointer_inside, None);
        assert_eq!(session.update_exposure(true, true, now), "hidden");
        assert_eq!(
            session.update_exposure(true, true, now + Duration::from_millis(150)),
            "reading"
        );
    }
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
        session.set_pointer(true, Instant::now());
        assert_eq!(session.update(false, now), "reading");
        session.set_pointer(false, Instant::now());
        assert_eq!(session.update(false, now), "hidden");
        assert_eq!(session.update(true, now), "hidden");
        assert_eq!(
            session.update(true, now + Duration::from_millis(150)),
            "reading"
        );
    }
    #[test]
    fn missed_pointer_leave_expires_and_hides_without_a_dom_event() {
        let mut session = ReaderSession::new("hover");
        let now = Instant::now();
        session.set_pointer(true, now);
        assert_eq!(session.update(false, now), "reading");
        assert_eq!(
            session.update(false, now + Duration::from_millis(89)),
            "reading"
        );
        assert_eq!(
            session.update(false, now + Duration::from_millis(90)),
            "hidden"
        );
        assert_eq!(session.pointer_inside, None);
        assert!(reader_mounted(&session.name, "hidden"));
        assert_eq!(
            session.update(true, now + Duration::from_millis(200)),
            "hidden"
        );
        assert_eq!(
            session.update(true, now + Duration::from_millis(350)),
            "reading"
        );
    }
    #[test]
    fn stationary_reader_stays_visible_after_hint_expiry_and_hides_on_global_exit() {
        let mut session = ReaderSession::new("hover");
        let now = Instant::now();
        session.set_pointer(true, now);
        assert_eq!(
            session.update(true, now + Duration::from_secs(60)),
            "reading"
        );
        assert_eq!(session.pointer_inside, None);
        assert_eq!(
            session.update(false, now + Duration::from_secs(61)),
            "hidden"
        );
        session.reset("ready", Some(true));
        assert_eq!(
            session.update(false, now + Duration::from_secs(120)),
            "ready"
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
        session.set_pointer(true, Instant::now());
        let now = Instant::now();
        let visible = session.update(false, now);
        assert_eq!(visible, "reading");
        assert!(reader_mounted(&session.name, visible));
        session.set_pointer(false, Instant::now());
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
