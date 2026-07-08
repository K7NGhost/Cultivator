use super::{FileFormatDetail, FileFormatPreview};

pub fn preview(path: &str, bytes: &[u8]) -> Option<FileFormatPreview> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(FileFormatPreview {
            kind: "jpeg".to_string(),
            label: "JPEG Image".to_string(),
            details: jpeg_image_details(bytes),
            media_path: Some(path.to_string()),
        });
    }

    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(FileFormatPreview {
            kind: "png".to_string(),
            label: "PNG Image".to_string(),
            details: png_image_details(bytes),
            media_path: Some(path.to_string()),
        });
    }

    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(FileFormatPreview {
            kind: "gif".to_string(),
            label: "GIF Image".to_string(),
            details: gif_image_details(bytes),
            media_path: Some(path.to_string()),
        });
    }

    if bytes.starts_with(b"BM") {
        return Some(FileFormatPreview {
            kind: "bmp".to_string(),
            label: "BMP Image".to_string(),
            details: bmp_image_details(bytes),
            media_path: Some(path.to_string()),
        });
    }

    None
}

fn png_image_details(bytes: &[u8]) -> Vec<FileFormatDetail> {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: "PNG".to_string(),
    }];

    if bytes.len() >= 24 {
        details.extend([
            FileFormatDetail {
                label: "Width".to_string(),
                value: u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]).to_string(),
            },
            FileFormatDetail {
                label: "Height".to_string(),
                value: u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]).to_string(),
            },
        ]);
    }

    details
}

fn gif_image_details(bytes: &[u8]) -> Vec<FileFormatDetail> {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: String::from_utf8_lossy(&bytes[..6.min(bytes.len())]).to_string(),
    }];

    if bytes.len() >= 10 {
        details.extend([
            FileFormatDetail {
                label: "Width".to_string(),
                value: u16::from_le_bytes([bytes[6], bytes[7]]).to_string(),
            },
            FileFormatDetail {
                label: "Height".to_string(),
                value: u16::from_le_bytes([bytes[8], bytes[9]]).to_string(),
            },
        ]);
    }

    details
}

fn bmp_image_details(bytes: &[u8]) -> Vec<FileFormatDetail> {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: "BMP".to_string(),
    }];

    if bytes.len() >= 26 {
        details.extend([
            FileFormatDetail {
                label: "Width".to_string(),
                value: i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]).to_string(),
            },
            FileFormatDetail {
                label: "Height".to_string(),
                value: i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]).to_string(),
            },
        ]);
    }

    details
}

fn jpeg_image_details(bytes: &[u8]) -> Vec<FileFormatDetail> {
    let mut details = vec![FileFormatDetail {
        label: "Magic".to_string(),
        value: "JPEG".to_string(),
    }];

    if let Some((width, height)) = jpeg_dimensions(bytes) {
        details.extend([
            FileFormatDetail {
                label: "Width".to_string(),
                value: width.to_string(),
            },
            FileFormatDetail {
                label: "Height".to_string(),
                value: height.to_string(),
            },
        ]);
    }

    details
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u16, u16)> {
    let mut index = 2;

    while index + 9 < bytes.len() {
        if bytes[index] != 0xff {
            index += 1;
            continue;
        }

        while index < bytes.len() && bytes[index] == 0xff {
            index += 1;
        }

        if index >= bytes.len() {
            return None;
        }

        let marker = bytes[index];
        index += 1;

        if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }

        if index + 2 > bytes.len() {
            return None;
        }

        let segment_length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;

        if segment_length < 2 || index + segment_length > bytes.len() {
            return None;
        }

        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            if index + 7 > bytes.len() {
                return None;
            }

            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]);
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);

            return Some((width, height));
        }

        index += segment_length;
    }

    None
}
