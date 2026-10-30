use crate::{changed, open_shelf, persist, set_mode, Store};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Bindings {
    pub toggle: String,
    pub shelf: String,
    pub adjust: String,
    pub previous: String,
    pub next: String,
}
impl Default for Bindings {
    fn default() -> Self {
        Self {
            toggle: "CommandOrControl+Shift+H".into(),
            shelf: "CommandOrControl+Shift+O".into(),
            adjust: "CommandOrControl+Shift+J".into(),
            previous: "CommandOrControl+Alt+Left".into(),
            next: "CommandOrControl+Alt+Right".into(),
        }
    }
}
pub fn entries(keys: &Bindings) -> [(&'static str, String); 5] {
    [
        ("toggle", keys.toggle.clone()),
        ("shelf", keys.shelf.clone()),
        ("adjust", keys.adjust.clone()),
        ("previous", keys.previous.clone()),
        ("next", keys.next.clone()),
    ]
}
fn validate(keys: &Bindings) -> Result<(), String> {
    let mut seen = HashSet::new();
    for (_, key) in entries(keys) {
        let shortcut: Shortcut = key.parse().map_err(|_| format!("无法识别快捷键：{key}"))?;
        if shortcut.mods.is_empty() || key.len() > 100 {
            return Err("全局快捷键需要包含 Ctrl、Command、Alt 或 Shift 等修饰键".into());
        }
        if !seen.insert(shortcut.id) {
            return Err("不同操作不能使用相同快捷键".into());
        }
    }
    Ok(())
}
fn register(app: &tauri::AppHandle, action: &'static str, key: &str) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(key, move |app, _, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            match action {
                "shelf" => open_shelf(app),
                "adjust" => {
                    let _ = set_mode(app.clone(), "setup".into());
                }
                "toggle" => {
                    let mode = app.state::<Store>().mode.lock().unwrap().name.clone();
                    let _ = set_mode(
                        app.clone(),
                        if mode == "paused" { "hover" } else { "paused" }.into(),
                    );
                }
                _ => {
                    let visible =
                        app.state::<Store>().presentation.lock().unwrap().state != "hidden";
                    if visible {
                        let _ = app.emit_to("reader", "reader-action", action);
                    }
                }
            }
        })
        .map_err(|e| format!("快捷键 {key} 注册失败，可能已被其他软件占用：{e}"))
}
pub fn install(app: &tauri::AppHandle, keys: &Bindings) {
    for (action, key) in entries(keys) {
        if let Err(error) = register(app, action, &key) {
            crate::diagnostics::record(
                "shortcut-registration-error",
                serde_json::json!({"action":action,"error":error}),
            );
        }
    }
}
#[tauri::command]
pub async fn save_shortcuts(app: tauri::AppHandle, bindings: Bindings) -> Result<(), String> {
    validate(&bindings)?;
    let store = app.state::<Store>();
    // A separate lock serializes edits without holding the library across native calls.
    let _guard = store.shortcut_edit.lock().await;
    let old = store.library.lock().unwrap().tools.shortcuts.clone();
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;
    for (action, key) in entries(&bindings) {
        if let Err(error) = register(&app, action, &key) {
            let _ = app.global_shortcut().unregister_all();
            install(&app, &old);
            return Err(error);
        }
    }
    let mut data = store.library.lock().unwrap();
    data.tools.shortcuts = bindings;
    if let Err(error) = persist(&store, &data) {
        data.tools.shortcuts = old.clone();
        drop(data);
        let _ = app.global_shortcut().unregister_all();
        install(&app, &old);
        return Err(error);
    }
    drop(data);
    changed(&app);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_keys_are_valid_and_conflicts_and_bare_keys_are_rejected() {
        let mut keys = Bindings::default();
        assert!(validate(&keys).is_ok());
        keys.next = keys.previous.clone();
        assert!(validate(&keys).unwrap_err().contains("相同"));
        keys.next = "A".into();
        assert!(validate(&keys).is_err());
    }
}
