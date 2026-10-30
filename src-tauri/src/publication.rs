use std::{fs::File, io::Read, path::Path};

pub(super) const MAX_BYTES: usize = 50 * 1024 * 1024;

pub(super) struct Publication {
    pub title: Option<String>,
    pub text: String,
    pub chapters: Vec<Chapter>,
}
#[derive(Clone, serde::Serialize)]
pub struct Chapter {
    pub title: String,
    pub offset: usize,
    pub level: usize,
}
pub fn txt_chapters(text: &str) -> Vec<Chapter> {
    let mut offset = 0;
    let mut chapters = vec![];
    for line in text.split_inclusive('\n') {
        let title = line.trim();
        let numbered = title
            .strip_prefix('第')
            .and_then(|s| {
                s.find(['章', '回', '卷', '节', '篇', '部'])
                    .map(|i| &s[..i])
            })
            .is_some_and(|n| {
                !n.is_empty()
                    && n.chars().all(|c| {
                        c.is_ascii_digit()
                            || "零〇一二三四五六七八九十百千万两壹贰叁肆伍陆柒捌玖拾佰仟"
                                .contains(c)
                    })
            });
        let special = ["序章", "序言", "楔子", "引子", "后记", "尾声", "番外"]
            .iter()
            .any(|prefix| title.starts_with(prefix));
        let english = title
            .to_ascii_lowercase()
            .strip_prefix("chapter ")
            .is_some_and(|n| {
                n.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit() || "ivxlcdm".contains(c))
            });
        if title.chars().count() <= 90 && (numbered || special || english) && chapters.len() < 20000
        {
            let leading = line.len() - line.trim_start().len();
            chapters.push(Chapter {
                title: title.into(),
                offset: offset + line[..leading].encode_utf16().count(),
                level: 0,
            });
        }
        offset += line.encode_utf16().count();
    }
    chapters
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
        let text = crate::text::decode(&raw)?;
        let chapters = txt_chapters(&text);
        Ok(Publication {
            title: None,
            text,
            chapters,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chapters_keep_utf16_positions_and_do_not_accept_prose() {
        let text = "😀开头\n第一章 起点\n正文里的第一章不是标题\n  Chapter 2 Next\n尾声\n";
        let chapters = txt_chapters(text);
        assert_eq!(chapters.len(), 3);
        let units: Vec<_> = text.encode_utf16().collect();
        for chapter in chapters {
            assert!(String::from_utf16_lossy(&units[chapter.offset..]).starts_with(&chapter.title));
        }
    }
}
