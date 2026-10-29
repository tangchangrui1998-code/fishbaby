use encoding_rs::{Encoding, GB18030};

const ENCODING_ERROR: &str =
    "无法解码此 TXT，请将文件另存为 UTF-8 后重新导入（支持 UTF-8、GB18030/GBK 和带 BOM 的 UTF-16）";

pub(super) fn decode(raw: &[u8]) -> Result<String, String> {
    // UTF-32LE shares the UTF-16LE prefix; don't mistake it for UTF-16.
    if raw.starts_with(&[0xff, 0xfe, 0, 0]) || raw.starts_with(&[0, 0, 0xfe, 0xff]) {
        return Err(ENCODING_ERROR.into());
    }
    let text = if let Some((encoding, bom_len)) = Encoding::for_bom(raw) {
        // An explicit BOM is authoritative. Never fall back after malformed Unicode.
        encoding.decode_without_bom_handling_and_without_replacement(&raw[bom_len..])
    } else if let Ok(text) = std::str::from_utf8(raw) {
        Some(text.into())
    } else {
        // GB18030 covers common mainland Chinese TXT files, including GBK/GB2312.
        GB18030.decode_without_bom_handling_and_without_replacement(raw)
    }
    .ok_or_else(|| ENCODING_ERROR.to_string())?;
    if text.contains('\0') {
        return Err(ENCODING_ERROR.into());
    }
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    if text.trim().is_empty() {
        return Err("文件中没有可阅读的文字".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_and_ascii_keep_text_and_normalize_line_endings() {
        assert_eq!(decode("\u{feff}甲😀\r\n乙".as_bytes()).unwrap(), "甲😀\n乙");
        assert_eq!(decode(b"Chapter 1\r\nHello").unwrap(), "Chapter 1\nHello");
    }

    #[test]
    fn gbk_and_gb18030_four_byte_characters_decode_without_loss() {
        assert_eq!(decode(&[0xd6, 0xd0, 0xce, 0xc4]).unwrap(), "中文");
        assert_eq!(decode(&[0x94, 0x39, 0xfc, 0x36]).unwrap(), "😀");
        let original = "第一章\r\n中文、𠀀和😀。";
        let (encoded, _, errors) = GB18030.encode(original);
        assert!(!errors);
        assert_eq!(decode(&encoded).unwrap(), original.replace("\r\n", "\n"));
    }

    #[test]
    fn utf16_both_byte_orders_support_surrogate_pairs() {
        for little_endian in [true, false] {
            let mut raw = if little_endian {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for unit in "甲😀\r\n乙".encode_utf16() {
                raw.extend(if little_endian {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            assert_eq!(decode(&raw).unwrap(), "甲😀\n乙");
        }
    }

    #[test]
    fn malformed_encodings_are_rejected_instead_of_replaced_or_reinterpreted() {
        for raw in [
            &[0x81][..],                            // Truncated GB18030.
            &[0xef, 0xbb, 0xbf, 0xd6, 0xd0][..],    // UTF-8 BOM with GBK payload.
            &[0xff, 0xfe, 0x41][..],                // Odd UTF-16 payload.
            &[0xff, 0xfe, 0, 0xd8][..],             // Unpaired UTF-16 surrogate.
            &[0xff, 0xfe, 0, 0, 0x41, 0, 0, 0][..], // UTF-32LE.
            &[0, 0, 0xfe, 0xff, 0, 0, 0, 0x41][..], // UTF-32BE.
            b"a\0b",
        ] {
            assert!(decode(raw).unwrap_err().contains("无法解码"));
        }
    }

    #[test]
    fn empty_text_in_each_encoding_is_rejected() {
        for raw in [
            &b""[..],
            &b" \r\n"[..],
            &[0xff, 0xfe][..],
            &[0xfe, 0xff, 0, 0x20][..],
        ] {
            assert!(decode(raw).unwrap_err().contains("没有可阅读"));
        }
    }
}
