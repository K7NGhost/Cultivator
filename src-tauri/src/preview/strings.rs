use super::{FileFormatDetail, FileFormatPreview};

pub fn sqlite_file_format_preview(bytes: &[u8]) -> FileFormatPreview {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: "SQLite format 3".to_string(),
    }];

    if bytes.len() >= 100 {
        let raw_page_size = u16::from_be_bytes([bytes[16], bytes[17]]);
        let page_size = if raw_page_size == 1 {
            65_536
        } else {
            raw_page_size as u32
        };
        let page_count = u32::from_be_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
        let schema_version = u32::from_be_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        let text_encoding = match u32::from_be_bytes([bytes[56], bytes[57], bytes[58], bytes[59]]) {
            1 => "UTF-8",
            2 => "UTF-16le",
            3 => "UTF-16be",
            _ => "Unknown",
        };

        details.extend([
            FileFormatDetail {
                label: "Page size".to_string(),
                value: format!("{page_size} bytes"),
            },
            FileFormatDetail {
                label: "Page count".to_string(),
                value: page_count.to_string(),
            },
            FileFormatDetail {
                label: "Schema version".to_string(),
                value: schema_version.to_string(),
            },
            FileFormatDetail {
                label: "Text encoding".to_string(),
                value: text_encoding.to_string(),
            },
        ]);
    }

    FileFormatPreview {
        kind: "sqlite".to_string(),
        label: "SQLite Database".to_string(),
        details,
        media_path: None,
    }
}
