use std::{fs::File, io::Read, path::Path};

pub(super) const MAX_BYTES: usize = 50 * 1024 * 1024;

pub(super) struct Publication {
    pub title: Option<String>,
    pub text: String,
}

pub(super) fn read(path: &str) -> Result<Publication, String> {
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "txt" | "epub") {
        return Err("请选择 TXT 或 EPUB 文件".into());
    }
    let file = File::open(path).map_err(|_| "文件不可用，请移出后重新导入".to_string())?;
    let mut raw = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|_| "文件读取失败".to_string())?;
    if raw.len() > MAX_BYTES {
        return Err("支持 50 MB 以内的 TXT 或 EPUB 文件".into());
    }
    if extension == "epub" {
        crate::epub::decode(&raw)
    } else {
        Ok(Publication {
            title: None,
            text: crate::text::decode(&raw)?,
        })
    }
}
