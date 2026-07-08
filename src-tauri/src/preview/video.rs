use super::{FileFormatDetail, FileFormatPreview};

pub fn preview(path: &str, bytes: &[u8]) -> Option<FileFormatPreview> {
    let Some(brand) = mp4_major_brand(bytes) else {
        return None;
    };

    Some(FileFormatPreview {
        kind: "mp4".to_string(),
        label: "MP4 Video".to_string(),
        details: vec![
            FileFormatDetail {
                label: "Container".to_string(),
                value: "ISO Base Media / MP4".to_string(),
            },
            FileFormatDetail {
                label: "Major brand".to_string(),
                value: brand,
            },
        ],
        media_path: Some(path.to_string()),
    })
}

fn mp4_major_brand(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 12 || &bytes[4..8] != b"ftyp" {
        return None;
    }

    let brand = &bytes[8..12];

    Some(String::from_utf8_lossy(brand).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mp4_file_type_box() {
        let preview =
            preview("sample.mp4", b"\0\0\0\x18ftypisom\0\0\0\0").expect("expected mp4 preview");

        assert_eq!(preview.kind, "mp4");
        assert_eq!(preview.media_path.as_deref(), Some("sample.mp4"));
    }

    #[test]
    fn ignores_non_mp4_bytes() {
        assert!(preview("sample.bin", b"not an mp4").is_none());
    }
}
