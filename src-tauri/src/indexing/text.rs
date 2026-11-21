pub(super) fn decode(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if !bytes.len().is_multiple_of(2) {
            return None;
        }
        let little = bytes[0] == 0xff;
        let words: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| {
                if little {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect();
        return String::from_utf16(&words).ok().filter(|s| is_text(s));
    }
    std::str::from_utf8(bytes)
        .ok()
        .filter(|s| is_text(s))
        .map(|s| s.trim_start_matches('\u{feff}').to_owned())
}

/// Probe only a small prefix before reading the rest of a candidate text file.
pub(super) fn read(path: &std::path::Path, limit: usize) -> Result<Option<String>, String> {
    let prefix = crate::evidence::read_prefix(path, 8192.min(limit + 1))?;
    let probe = match std::str::from_utf8(&prefix) {
        Err(error) if error.error_len().is_none() => &prefix[..error.valid_up_to()],
        _ => &prefix,
    };
    if decode(probe).is_none() {
        return Ok(None);
    }
    if prefix.len() < 8192 {
        return Ok(decode(&prefix));
    }
    let bytes = crate::evidence::read_prefix(path, limit + 1)?;
    if bytes.len() > limit {
        Ok(None)
    } else {
        Ok(decode(&bytes))
    }
}

fn is_text(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{c}'))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_unicode_and_bom_but_rejects_binary() {
        assert_eq!(decode("hello 🌱".as_bytes()).as_deref(), Some("hello 🌱"));
        assert_eq!(decode(&[255, 254, 104, 0, 105, 0]).as_deref(), Some("hi"));
        assert!(decode(b"abc\0def").is_none());
        assert!(decode(&[255, 254, 1]).is_none());
    }
}
