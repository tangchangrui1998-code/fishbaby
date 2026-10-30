use crate::publication::{Chapter, Publication, MAX_BYTES};
use encoding_rs::Encoding;
use percent_encoding::percent_decode_str;
use roxmltree::Document;
use scraper::{ElementRef, Html, Node};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
};
use zip::{result::ZipError, ZipArchive};

const INVALID: &str = "EPUB 文件损坏或结构不完整，请重新下载后导入";
const TOO_LARGE: &str = "EPUB 解压后的正文及目录不能超过 50 MB";

struct Archive<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    remaining: usize,
}
impl Archive<'_> {
    fn optional(&mut self, name: &str) -> Result<Option<String>, String> {
        let mut entry = match self.zip.by_name(name) {
            Ok(entry) => entry,
            Err(ZipError::FileNotFound) => return Ok(None),
            Err(_) => return Err(INVALID.into()),
        };
        if entry.encrypted() {
            return Err("暂不支持加密或带 DRM 的 EPUB，请使用未加密版本".into());
        }
        if entry.size() > self.remaining as u64 {
            return Err(TOO_LARGE.into());
        }
        let mut raw = Vec::new();
        (&mut entry)
            .take((self.remaining + 1) as u64)
            .read_to_end(&mut raw)
            .map_err(|_| INVALID.to_string())?;
        if raw.len() > self.remaining {
            return Err(TOO_LARGE.into());
        }
        self.remaining -= raw.len();
        let (encoding, start) = Encoding::for_bom(&raw).unwrap_or((encoding_rs::UTF_8, 0));
        let text = encoding
            .decode_without_bom_handling_and_without_replacement(&raw[start..])
            .ok_or_else(|| "EPUB 正文编码无效，应为 UTF-8 或 UTF-16".to_string())?;
        Ok(Some(text.into_owned()))
    }
    fn required(&mut self, name: &str) -> Result<String, String> {
        self.optional(name)?.ok_or_else(|| INVALID.to_string())
    }
}

// Resolve only archive-relative resource paths; never extract files or access the network.
fn resource_path(base: &str, href: &str) -> Result<String, String> {
    let href = href.split(['#', '?']).next().unwrap_or_default();
    let decoded = percent_decode_str(href)
        .decode_utf8()
        .map_err(|_| INVALID.to_string())?;
    if decoded.is_empty() || decoded.starts_with('/') || decoded.contains([':', '\\', '\0']) {
        return Err(INVALID.into());
    }
    let mut parts: Vec<_> = base
        .rsplit_once('/')
        .map(|(dir, _)| dir.split('/').collect())
        .unwrap_or_default();
    for part in decoded.split('/') {
        match part {
            "." | "" => (),
            ".." => {
                parts.pop().ok_or_else(|| INVALID.to_string())?;
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

pub(super) fn decode(raw: &[u8]) -> Result<Publication, String> {
    let zip = ZipArchive::new(Cursor::new(raw)).map_err(|_| INVALID.to_string())?;
    if zip.len() > 10_000 {
        return Err("EPUB 内文件数量过多".into());
    }
    let mut archive = Archive {
        zip,
        remaining: MAX_BYTES,
    };
    if archive.required("mimetype")?.trim() != "application/epub+zip" {
        return Err(INVALID.into());
    }
    if let Some(encryption) = archive.optional("META-INF/encryption.xml")? {
        let doc = Document::parse(&encryption).map_err(|_| INVALID.to_string())?;
        // EPUB font obfuscation is common and doesn't encrypt the reading text.
        for method in doc
            .descendants()
            .filter(|n| n.has_tag_name("EncryptionMethod"))
        {
            if !matches!(
                method.attribute("Algorithm"),
                Some("http://www.idpf.org/2008/embedding" | "http://ns.adobe.com/pdf/enc#RC")
            ) {
                return Err("暂不支持加密或带 DRM 的 EPUB，请使用未加密版本".into());
            }
        }
    }
    let container = archive.required("META-INF/container.xml")?;
    let doc = Document::parse(&container).map_err(|_| INVALID.to_string())?;
    let rootfiles: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name("rootfile"))
        .collect();
    let package = rootfiles
        .iter()
        .find(|n| n.attribute("media-type") == Some("application/oebps-package+xml"))
        .or_else(|| rootfiles.first())
        .and_then(|n| n.attribute("full-path"))
        .ok_or_else(|| INVALID.to_string())?;
    let package = resource_path("", package)?;
    let opf = archive.required(&package)?;
    let doc = Document::parse(&opf).map_err(|_| INVALID.to_string())?;
    let root = doc.root_element();
    let metadata = root.children().find(|n| n.has_tag_name("metadata"));
    let title = metadata
        .and_then(|n| n.children().find(|n| n.has_tag_name("title")))
        .and_then(|n| n.text())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let manifest = root
        .children()
        .find(|n| n.has_tag_name("manifest"))
        .ok_or_else(|| INVALID.to_string())?;
    let items: HashMap<_, _> = manifest
        .children()
        .filter(|n| n.has_tag_name("item"))
        .filter_map(|n| Some((n.attribute("id")?, n)))
        .collect();
    let spine = root
        .children()
        .find(|n| n.has_tag_name("spine"))
        .ok_or_else(|| INVALID.to_string())?;
    let mut text = String::new();
    let mut locations = HashMap::new();
    let mut fallback = vec![];
    let mut units = 0;
    for reference in spine.children().filter(|n| n.has_tag_name("itemref")) {
        if reference.attribute("linear") == Some("no") {
            continue;
        }
        let item = reference
            .attribute("idref")
            .and_then(|id| items.get(id))
            .ok_or_else(|| INVALID.to_string())?;
        if item
            .attribute("properties")
            .is_some_and(|p| p.split_whitespace().any(|p| p == "nav"))
        {
            continue;
        }
        // Covers can be SVG resources. Text novels use XHTML spine documents.
        if item.attribute("media-type") == Some("image/svg+xml") {
            continue;
        }
        if !matches!(
            item.attribute("media-type"),
            Some("application/xhtml+xml" | "text/html")
        ) {
            return Err("此 EPUB 包含暂不支持的正文格式".into());
        }
        let href = item.attribute("href").ok_or_else(|| INVALID.to_string())?;
        let path = resource_path(&package, href)?;
        let source = archive.required(&path)?;
        let (chapter, anchors) = chapter_with_anchors(&source)?;
        if chapter.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push_str("\n\n");
            units += 2;
        }
        locations.insert(path.clone(), units);
        for (anchor, offset) in anchors {
            locations.insert(format!("{path}#{anchor}"), units + offset);
        }
        fallback.push(Chapter {
            title: chapter
                .lines()
                .next()
                .unwrap_or("未命名章节")
                .chars()
                .take(90)
                .collect(),
            offset: units,
            level: 0,
        });
        units += chapter.encode_utf16().count();
        text.push_str(&chapter);
    }
    if text.is_empty() {
        return Err("此 EPUB 没有可阅读的正文，纯图片书暂不支持".into());
    }
    let mut chapters = vec![];
    let nav = items.values().find(|item| {
        item.attribute("properties")
            .is_some_and(|p| p.split_whitespace().any(|p| p == "nav"))
    });
    let ncx = spine
        .attribute("toc")
        .and_then(|id| items.get(id))
        .copied()
        .or_else(|| {
            items
                .values()
                .find(|item| item.attribute("media-type") == Some("application/x-dtbncx+xml"))
                .copied()
        });
    if let Some(item) = nav.or(ncx.as_ref()) {
        if let Some(href) = item.attribute("href") {
            let toc_path = resource_path(&package, href)?;
            if let Some(source) = archive.optional(&toc_path)? {
                let links: Vec<(String, String, usize)> = if nav.is_some() {
                    let html = Html::parse_document(&source);
                    let nav_selector = scraper::Selector::parse("nav").unwrap();
                    let selected = html.select(&nav_selector).find(|e| {
                        e.value()
                            .attr("epub:type")
                            .is_some_and(|t| t.split_whitespace().any(|t| t == "toc"))
                            || e.value().attr("role") == Some("doc-toc")
                    });
                    let anchors = scraper::Selector::parse("a[href]").unwrap();
                    selected
                        .map(|nav| {
                            nav.select(&anchors)
                                .map(|a| {
                                    (
                                        a.value().attr("href").unwrap().into(),
                                        a.text().collect::<String>().trim().into(),
                                        a.ancestors()
                                            .filter_map(ElementRef::wrap)
                                            .filter(|e| e.value().name() == "ol")
                                            .count()
                                            .saturating_sub(1),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                } else if let Ok(doc) = Document::parse(&source) {
                    doc.descendants()
                        .filter(|n| n.has_tag_name("navPoint"))
                        .filter_map(|n| {
                            let href = n
                                .children()
                                .find(|c| c.has_tag_name("content"))?
                                .attribute("src")?;
                            let label = n
                                .children()
                                .find(|c| c.has_tag_name("navLabel"))?
                                .descendants()
                                .find(|c| c.has_tag_name("text"))?
                                .text()?;
                            Some((
                                href.into(),
                                label.trim().into(),
                                n.ancestors()
                                    .filter(|a| a.has_tag_name("navPoint"))
                                    .count()
                                    .saturating_sub(1),
                            ))
                        })
                        .collect()
                } else {
                    vec![]
                };
                for (href, label, level) in links.into_iter().take(20000) {
                    let raw_path = href.split('#').next().unwrap_or("");
                    let target = if raw_path.is_empty() {
                        toc_path.clone()
                    } else {
                        match resource_path(&toc_path, raw_path) {
                            Ok(path) => path,
                            Err(_) => continue,
                        }
                    };
                    let anchor = href
                        .split_once('#')
                        .map(|(_, a)| percent_decode_str(a).decode_utf8_lossy().to_string());
                    let key = anchor.map(|a| format!("{target}#{a}")).unwrap_or(target);
                    if let Some(&offset) = locations.get(&key) {
                        if !label.is_empty() {
                            chapters.push(Chapter {
                                title: label,
                                offset,
                                level: level.min(8),
                            });
                        }
                    }
                }
            }
        }
    }
    if chapters.is_empty() {
        chapters = fallback;
    }
    chapters.sort_by_key(|c| c.offset);
    Ok(Publication {
        title,
        text,
        chapters,
    })
}

fn line_break(text: &mut String) {
    while text.ends_with(' ') {
        text.pop();
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
}
fn append_element(
    element: ElementRef<'_>,
    text: &mut String,
    anchors: &mut HashMap<String, usize>,
    depth: usize,
) -> Result<(), String> {
    if depth > 256 {
        return Err("EPUB 正文嵌套过深，无法读取".into());
    }
    let value = element.value();
    if matches!(
        value.name(),
        "head" | "script" | "style" | "noscript" | "nav" | "svg" | "rt" | "rp"
    ) || value.attr("hidden").is_some()
        || value.attr("aria-hidden") == Some("true")
    {
        return Ok(());
    }
    let block = matches!(
        value.name(),
        "p" | "div"
            | "section"
            | "article"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "li"
            | "blockquote"
            | "pre"
            | "tr"
            | "dt"
            | "dd"
            | "br"
            | "hr"
    );
    if block {
        line_break(text);
    }
    if let Some(id) = value.attr("id").or_else(|| value.attr("name")) {
        anchors.entry(id.into()).or_insert(text.len());
    }
    for child in element.children() {
        match child.value() {
            Node::Text(content) => {
                for c in content.text.chars() {
                    if c.is_whitespace() {
                        if !text.is_empty() && !text.ends_with([' ', '\n']) {
                            text.push(' ');
                        }
                    } else {
                        text.push(c);
                    }
                }
            }
            Node::Element(_) => {
                append_element(ElementRef::wrap(child).unwrap(), text, anchors, depth + 1)?
            }
            _ => (),
        }
    }
    if block {
        line_break(text);
    }
    Ok(())
}
#[cfg(test)]
fn chapter_text(source: &str) -> Result<String, String> {
    chapter_with_anchors(source).map(|(text, _)| text)
}
fn chapter_with_anchors(source: &str) -> Result<(String, HashMap<String, usize>), String> {
    let html = Html::parse_document(source);
    let mut text = String::new();
    let mut anchors = HashMap::new();
    append_element(html.root_element(), &mut text, &mut anchors, 0)?;
    let leading = text.len() - text.trim_start().len();
    let trimmed = text.trim().to_string();
    let mut targets: Vec<_> = anchors
        .into_iter()
        .map(|(id, byte)| (byte.saturating_sub(leading).min(trimmed.len()), id))
        .collect();
    targets.sort_by_key(|(byte, _)| *byte);
    let mut result = HashMap::new();
    let mut targets = targets.into_iter().peekable();
    let mut units = 0;
    for (byte, c) in trimmed.char_indices() {
        while targets.peek().is_some_and(|(target, _)| *target <= byte) {
            let (_, id) = targets.next().unwrap();
            result.insert(id, units);
        }
        units += c.len_utf16();
    }
    for (_, id) in targets {
        result.insert(id, units);
    }
    Ok((trimmed, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{write::SimpleFileOptions, ZipWriter};

    fn fixture(opf: &str, extras: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let container = r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        for (name, content) in [
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", container),
            ("OPS/package.opf", opf),
        ]
        .into_iter()
        .chain(extras.iter().copied())
        {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    fn package(version: &str) -> String {
        format!(
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="{version}"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>测试书 &amp; 小说</dc:title></metadata><manifest><item id="second" href="text/a.xhtml" media-type="application/xhtml+xml"/><item id="first" href="../text/第一%20章.xhtml#start" media-type="application/xhtml+xml"/><item id="notes" href="missing.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="first"/><itemref idref="notes" linear="no"/><itemref idref="second"/></spine></package>"#
        )
    }
    #[test]
    fn native_epub3_toc_and_epub2_ncx_resolve_fragment_offsets_after_emoji() {
        for epub3 in [true, false] {
            let toc_item = if epub3 {
                r#"<item id="toc" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#
            } else {
                r#"<item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#
            };
            let opf = format!(
                r#"<package><metadata/><manifest><item id="body" href="body.xhtml" media-type="application/xhtml+xml"/>{toc_item}</manifest><spine toc="toc"><itemref idref="body"/></spine></package>"#
            );
            let toc = if epub3 {
                r##"<html xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="body.xhtml#b">第二节</a></li></ol></nav></body></html>"##
            } else {
                r##"<ncx><navMap><navPoint><navLabel><text>第二节</text></navLabel><content src="body.xhtml#b"/></navPoint></navMap></ncx>"##
            };
            let path = if epub3 {
                "OPS/nav.xhtml"
            } else {
                "OPS/toc.ncx"
            };
            let raw = fixture(
                &opf,
                &[
                    (
                        "OPS/body.xhtml",
                        r#"<html><body><p>😀开头</p><h2 id="b">真正标题</h2><p>正文</p></body></html>"#,
                    ),
                    (path, toc),
                ],
            );
            let publication = decode(&raw).unwrap();
            assert_eq!(publication.chapters.len(), 1);
            assert_eq!(publication.chapters[0].title, "第二节");
            let units: Vec<_> = publication.text.encode_utf16().collect();
            assert!(
                String::from_utf16_lossy(&units[publication.chapters[0].offset..])
                    .starts_with("真正标题")
            );
        }
    }
    #[test]
    fn epub2_and_epub3_use_spine_order_metadata_and_relative_unicode_paths() {
        for version in ["2.0", "3.0"] {
            let raw = fixture(&package(version), &[
                ("OPS/text/a.xhtml", "<html><body><h2>第二章</h2><p>尾声😀</p></body></html>"),
                ("text/第一 章.xhtml", "<html><head><title>不应混入的标题</title><style>css</style></head><body><h1>第一章</h1><p>甲<strong>乙</strong>&amp;丙&nbsp;丁<br/>换行</p><script>javascript</script><p hidden>隐藏</p></body></html>"),
            ]);
            let book = decode(&raw).unwrap();
            assert_eq!(book.title.as_deref(), Some("测试书 & 小说"));
            assert_eq!(book.text, "第一章\n甲乙&丙 丁\n换行\n\n第二章\n尾声😀");
            assert_eq!(decode(&raw).unwrap().text, book.text);
            let path = std::env::temp_dir().join(format!(
                "fishbaby-epub-{version}-{}.EPUB",
                std::process::id()
            ));
            std::fs::write(&path, &raw).unwrap();
            let imported = crate::publication::read(path.to_str().unwrap()).unwrap();
            assert_eq!(imported.title, book.title);
            assert_eq!(imported.text, book.text);
            assert_eq!(std::fs::read(&path).unwrap(), raw);
            std::fs::remove_file(path).unwrap();
        }
    }
    #[test]
    fn missing_or_corrupted_resources_fail_instead_of_silently_losing_chapters() {
        assert!(decode(b"not a zip").err().unwrap().contains("损坏"));
        assert!(decode(&fixture(&package("3.0"), &[]))
            .err()
            .unwrap()
            .contains("损坏"));
        assert!(decode(&fixture("<package>", &[]))
            .err()
            .unwrap()
            .contains("损坏"));
    }
    #[test]
    fn empty_and_image_only_books_report_no_readable_text() {
        let opf = r#"<package><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine></package>"#;
        let raw = fixture(
            opf,
            &[(
                "OPS/a.xhtml",
                "<html><body><img src='cover.png'/></body></html>",
            )],
        );
        assert!(decode(&raw).err().unwrap().contains("没有可阅读"));
    }
    #[test]
    fn drm_is_rejected_but_obfuscated_fonts_do_not_block_reading() {
        let opf = r#"<package><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine></package>"#;
        for (algorithm, accepted) in [
            ("http://www.w3.org/2001/04/xmlenc#aes128-cbc", false),
            ("http://www.idpf.org/2008/embedding", true),
            ("http://ns.adobe.com/pdf/enc#RC", true),
        ] {
            let encryption = format!(
                r#"<encryption><EncryptedData><EncryptionMethod Algorithm="{algorithm}"/></EncryptedData></encryption>"#
            );
            let raw = fixture(
                opf,
                &[
                    ("META-INF/encryption.xml", &encryption),
                    ("OPS/a.xhtml", "<p>正文</p>"),
                ],
            );
            let book = decode(&raw);
            if accepted {
                assert_eq!(book.unwrap().text, "正文");
            } else {
                assert!(book.err().unwrap().contains("DRM"));
            }
        }
    }
    #[test]
    fn archive_reads_are_bounded_and_paths_cannot_escape_the_archive() {
        let raw = fixture(&package("3.0"), &[]);
        let mut archive = Archive {
            zip: ZipArchive::new(Cursor::new(raw.as_slice())).unwrap(),
            remaining: 4,
        };
        assert!(archive.required("mimetype").unwrap_err().contains("50 MB"));
        for href in [
            "../../outside",
            "https://example.com/chapter",
            "/absolute",
            "..%2F..%2Foutside",
            "a\\b",
        ] {
            assert!(resource_path("OPS/package.opf", href).is_err());
        }
        assert_eq!(
            resource_path("OPS/package.opf", "../text/a.xhtml").unwrap(),
            "text/a.xhtml"
        );
    }
    #[test]
    fn headings_paragraphs_entities_and_inline_text_are_preserved() {
        assert_eq!(chapter_text("<!DOCTYPE html><html><body><h1>标题</h1>\n<p>你<em>好</em>，&lt;世界&gt;。</p><p>下一段</p><nav>目录</nav></body></html>").unwrap(), "标题\n你好，<世界>。\n下一段");
    }
}
