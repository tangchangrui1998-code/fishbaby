pub mod displays;
pub mod shortcuts;
use crate::{changed, now, persist, publication, Store};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Write};
use tauri::Manager;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReadingTools {
    pub bookmarks: HashMap<String, Vec<Bookmark>>,
    pub layouts: Vec<displays::Layout>,
    pub shortcuts: shortcuts::Bindings,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bookmark {
    pub id: String,
    pub offset: usize,
    pub anchor: String,
    pub label: String,
    pub created_at: u64,
}
#[tauri::command]
pub async fn reading_navigation(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let store = app.state::<Store>();
    let (book, marks) = {
        let data = store.library.lock().unwrap();
        let book = data
            .books
            .iter()
            .find(|b| Some(&b.id) == data.active_id.as_ref())
            .ok_or("请先选择一本书")?
            .clone();
        let marks = data
            .tools
            .bookmarks
            .get(&book.id)
            .cloned()
            .unwrap_or_default();
        (book, marks)
    };
    let publication = publication::read(&book.path)?;
    let length = publication.text.encode_utf16().count();
    let units: Vec<_> = if marks.is_empty() {
        vec![]
    } else {
        publication.text.encode_utf16().collect()
    };
    let bookmarks: Vec<_> = marks
        .into_iter()
        .map(|mut mark| {
            mark.offset = relocate(&publication.text, &units, mark.offset, &mark.anchor);
            mark.offset = mark.offset.min(length.saturating_sub(1));
            mark
        })
        .collect();
    Ok(
        serde_json::json!({"bookId": book.id, "chapters": publication.chapters, "bookmarks": bookmarks}),
    )
}
#[tauri::command]
pub async fn save_bookmark(
    app: tauri::AppHandle,
    book_id: String,
    offset: usize,
    label: String,
) -> Result<(), String> {
    let store = app.state::<Store>();
    let path = store
        .library
        .lock()
        .unwrap()
        .books
        .iter()
        .find(|b| b.id == book_id)
        .ok_or("书籍不存在")?
        .path
        .clone();
    let text = publication::read(&path)?.text;
    let units: Vec<_> = text.encode_utf16().collect();
    let offset = offset.min(units.len().saturating_sub(1));
    let offset = if (0xDC00..=0xDFFF).contains(&units[offset]) {
        offset.saturating_sub(1)
    } else {
        offset
    };
    let anchor: String = char::decode_utf16(units[offset..].iter().copied())
        .take(80)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    let label: String = label.trim().chars().take(80).collect();
    let label = if label.is_empty() {
        anchor.chars().take(24).collect()
    } else {
        label
    };
    let mut data = store.library.lock().unwrap();
    if !data.books.iter().any(|b| b.id == book_id) {
        return Err("书籍已移出".into());
    }
    let marks = data.tools.bookmarks.entry(book_id).or_default();
    if let Some(mark) = marks.iter_mut().find(|m| m.offset == offset) {
        mark.label = label;
    } else {
        if marks.len() >= 1000 {
            return Err("每本书最多保存 1000 个书签".into());
        }
        marks.push(Bookmark {
            id: format!("{}-{offset}-{}", now(), marks.len()),
            offset,
            anchor,
            label,
            created_at: now(),
        });
    }
    persist(&store, &data)?;
    drop(data);
    changed(&app);
    Ok(())
}
#[tauri::command]
pub fn remove_bookmark(app: tauri::AppHandle, book_id: String, id: String) -> Result<(), String> {
    let store = app.state::<Store>();
    let mut data = store.library.lock().unwrap();
    if let Some(marks) = data.tools.bookmarks.get_mut(&book_id) {
        marks.retain(|m| m.id != id);
    }
    persist(&store, &data)?;
    drop(data);
    changed(&app);
    Ok(())
}
#[tauri::command]
pub fn app_information(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let data = app.state::<Store>().library.lock().unwrap().tools.clone();
    let bindings = shortcuts::entries(&data.shortcuts);
    Ok(serde_json::json!({
        "version": app.package_info().version.to_string(), "preview": true,
        "revision": env!("FISHBABY_REVISION"), "builtAt": env!("FISHBABY_BUILT_AT"),
        "platform": std::env::consts::OS, "architecture": std::env::consts::ARCH,
        "logDirectory": app.path().app_log_dir().map_err(|e| e.to_string())?,
        "shortcuts": bindings.iter().map(|(action,key)| serde_json::json!({"action": action, "key": key, "active": app.global_shortcut().is_registered(key.as_str())})).collect::<Vec<_>>(),
        "layouts": data.layouts,
    }))
}
use tauri_plugin_global_shortcut::GlobalShortcutExt;
#[tauri::command]
pub async fn open_log_directory(app: tauri::AppHandle) -> Result<(), String> {
    let path = app.path().app_log_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(windows)]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", windows)))]
    let program = "xdg-open";
    let mut child = std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
#[tauri::command]
pub async fn export_diagnostics(app: tauri::AppHandle, destination: String) -> Result<(), String> {
    let directory = app.path().app_log_dir().map_err(|e| e.to_string())?;
    let info = app_information(app.clone())?;
    // Export environment metadata only: no library, book paths, titles, shortcuts or layouts.
    let metadata = serde_json::json!({"version": info["version"], "preview": info["preview"], "revision": info["revision"], "builtAt": info["builtAt"], "platform": info["platform"], "architecture": info["architecture"]});
    let bytes = diagnostic_archive(&directory, &metadata)?;
    fs::write(destination, bytes).map_err(|e| e.to_string())
}
fn diagnostic_archive(
    directory: &std::path::Path,
    metadata: &serde_json::Value,
) -> Result<Vec<u8>, String> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut buffer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        archive
            .start_file("environment.json", options)
            .map_err(|e| e.to_string())?;
        archive
            .write_all(metadata.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        for name in [
            "reader.jsonl",
            "reader.1.jsonl",
            "reader.2.jsonl",
            "reader.3.jsonl",
        ] {
            if let Ok(bytes) = fs::read(directory.join(name)) {
                archive
                    .start_file(name, options)
                    .map_err(|e| e.to_string())?;
                archive.write_all(&bytes).map_err(|e| e.to_string())?;
            }
        }
        archive.finish().map_err(|e| e.to_string())?;
    }
    Ok(buffer.into_inner())
}

fn relocate(text: &str, units: &[u16], offset: usize, anchor: &str) -> usize {
    if anchor.is_empty() {
        return offset;
    }
    let target: Vec<_> = anchor.encode_utf16().collect();
    if units.get(offset..offset.saturating_add(target.len())) == Some(target.as_slice()) {
        return offset;
    }
    let mut previous_byte = 0;
    let mut position: usize = 0;
    let mut nearest = None;
    for (byte, _) in text.match_indices(anchor) {
        position += text[previous_byte..byte].encode_utf16().count();
        previous_byte = byte;
        if nearest.is_none_or(|old: usize| position.abs_diff(offset) < old.abs_diff(offset)) {
            nearest = Some(position);
        }
    }
    nearest.unwrap_or(offset)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_export_includes_only_whitelisted_logs_and_environment() {
        let directory =
            std::env::temp_dir().join(format!("fishbaby-export-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("library.json"), "private book names").unwrap();
        fs::write(directory.join("reader.jsonl"), "{\"event\":\"hidden\"}\n").unwrap();
        let bytes = diagnostic_archive(&directory, &serde_json::json!({"version":"test"})).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 2);
        assert!(archive.by_name("environment.json").is_ok());
        assert!(archive.by_name("reader.jsonl").is_ok());
        assert!(archive.by_name("library.json").is_err());
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn bookmarks_keep_duplicate_passages_and_relocate_after_edits() {
        let text = "😀重复\n其他内容\n重复";
        let units: Vec<_> = text.encode_utf16().collect();
        let last = units.len() - 2;
        assert_eq!(relocate(text, &units, last, "重复"), last);
        let edited = format!("新内容{text}");
        let updated: Vec<_> = edited.encode_utf16().collect();
        assert_eq!(relocate(&edited, &updated, last, "重复"), last + 3);
    }
    #[test]
    fn old_libraries_supply_all_new_tool_defaults() {
        let library: crate::Library = serde_json::from_str("{}").unwrap();
        assert!(library.tools.bookmarks.is_empty());
        assert!(library.tools.layouts.is_empty());
        assert!(library.tools.shortcuts.toggle.contains("Shift"));
    }
}
