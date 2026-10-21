//! Apple-optimized PNGs use raw DEFLATE and premultiplied BGR(A) pixels.
//! Decode scanlines directly so thumbnail generation never needs Python or
//! a second compressed PNG. Adam7 passes share the same bounded row buffers.
use super::*;
use flate2::read::DeflateDecoder;

pub(super) fn decode(
    source: &Source,
    cancelled: &AtomicBool,
    budget: u64,
) -> Result<DynamicImage, NativeError> {
    let mut data = Vec::new();
    fs::File::open(&source.path)?
        .take(source.size.saturating_add(1))
        .read_to_end(&mut data)?;
    if data.len() as u64 != source.size {
        return Err("Media source changed during decoding.".to_string().into());
    }
    if !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Invalid CgBI PNG signature.".to_string().into());
    }
    let mut position = 8usize;
    let mut header = None;
    let mut compressed = Vec::new();
    let mut ended = false;
    let mut orientation = image::metadata::Orientation::NoTransforms;
    while position < data.len() {
        check_cancel(cancelled)?;
        let prefix = data
            .get(position..position.saturating_add(8))
            .ok_or_else(|| NativeError::Failed("Truncated PNG chunk.".into()))?;
        let length = u32::from_be_bytes(prefix[..4].try_into().unwrap()) as usize;
        let kind = &prefix[4..8];
        let end = position
            .checked_add(12)
            .and_then(|p| p.checked_add(length))
            .ok_or_else(|| NativeError::Failed("Invalid PNG chunk length.".into()))?;
        let chunk = data
            .get(position + 8..end.saturating_sub(4))
            .ok_or_else(|| NativeError::Failed("Truncated PNG chunk data.".into()))?;
        let checksum = data
            .get(end - 4..end)
            .ok_or_else(|| NativeError::Failed("Missing PNG checksum.".into()))?;
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(chunk);
        if crc.finalize() != u32::from_be_bytes(checksum.try_into().unwrap()) {
            return Err("Invalid CgBI PNG checksum.".to_string().into());
        }
        match kind {
            b"IHDR" => {
                if header.is_some() || chunk.len() != 13 {
                    return Err("Invalid CgBI PNG header.".to_string().into());
                }
                header = Some(chunk.to_vec());
            }
            b"IDAT" => compressed.extend_from_slice(chunk),
            b"eXIf" => {
                if let Ok(exif) = exif::Reader::new().read_raw(chunk.to_vec()) {
                    if let Some(value) = exif
                        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                        .and_then(|field| field.value.get_uint(0))
                    {
                        orientation = image::metadata::Orientation::from_exif(value as u8)
                            .unwrap_or(orientation);
                    }
                }
            }
            b"IEND" => {
                ended = true;
                break;
            }
            _ => {}
        }
        position = end;
    }
    let header = header.ok_or_else(|| NativeError::Failed("Missing CgBI PNG header.".into()))?;
    if !ended || compressed.is_empty() {
        return Err("Incomplete CgBI PNG.".to_string().into());
    }
    let width = u32::from_be_bytes(header[..4].try_into().unwrap());
    let height = u32::from_be_bytes(header[4..8].try_into().unwrap());
    if header[8] != 8
        || !matches!(header[9], 2 | 6)
        || header[10] != 0
        || header[11] != 0
        || header[12] > 1
    {
        return Err(NativeError::Unsupported);
    }
    check_budget(
        width,
        height,
        u64::from(width) * u64::from(height) * 3,
        source.size,
        budget,
    )?;
    let channels = if header[9] == 6 { 4usize } else { 3 };
    let passes: &[(u32, u32, u32, u32)] = if header[12] == 0 {
        &[(0, 0, 1, 1)]
    } else {
        &[
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ]
    };
    let mut decoder = DeflateDecoder::new(compressed.as_slice());
    let mut pixels = vec![0u8; width as usize * height as usize * 3];
    for &(x0, y0, dx, dy) in passes {
        let pass_width = width.saturating_sub(x0).div_ceil(dx);
        let pass_height = height.saturating_sub(y0).div_ceil(dy);
        if pass_width == 0 || pass_height == 0 {
            continue;
        }
        let row_length = pass_width as usize * channels;
        let mut previous = vec![0u8; row_length];
        let mut row = vec![0u8; row_length];
        for py in 0..pass_height {
            check_cancel(cancelled)?;
            let mut filter = [0u8; 1];
            decoder.read_exact(&mut filter)?;
            decoder.read_exact(&mut row)?;
            if filter[0] > 4 {
                return Err("Invalid CgBI PNG row filter.".to_string().into());
            }
            for index in 0..row.len() {
                let left = if index >= channels {
                    row[index - channels]
                } else {
                    0
                };
                let above = previous[index];
                let upper_left = if index >= channels {
                    previous[index - channels]
                } else {
                    0
                };
                let predictor = match filter[0] {
                    0 => 0,
                    1 => left,
                    2 => above,
                    3 => ((u16::from(left) + u16::from(above)) / 2) as u8,
                    _ => paeth(left, above, upper_left),
                };
                row[index] = row[index].wrapping_add(predictor);
            }
            for px in 0..pass_width {
                let input = px as usize * channels;
                let output =
                    (((y0 + py * dy) as usize * width as usize) + (x0 + px * dx) as usize) * 3;
                let alpha = if channels == 4 {
                    u32::from(row[input + 3])
                } else {
                    255
                };
                for channel in 0..3 {
                    let component = u32::from(row[input + 2 - channel]);
                    pixels[output + channel] = if alpha > 0 {
                        ((component * 255 + alpha / 2) / alpha).min(255) as u8
                    } else {
                        component as u8
                    };
                }
            }
            std::mem::swap(&mut row, &mut previous);
        }
    }
    let mut extra = [0u8; 1];
    if decoder.read(&mut extra)? != 0 {
        return Err("CgBI PNG has excess pixel data.".to_string().into());
    }
    let mut image = DynamicImage::ImageRgb8(
        image::RgbImage::from_raw(width, height, pixels)
            .ok_or_else(|| NativeError::Failed("Invalid CgBI pixels.".into()))?,
    );
    image.apply_orientation(orientation);
    Ok(image)
}

fn paeth(left: u8, above: u8, upper_left: u8) -> u8 {
    let p = i32::from(left) + i32::from(above) - i32::from(upper_left);
    let a = (p - i32::from(left)).abs();
    let b = (p - i32::from(above)).abs();
    let c = (p - i32::from(upper_left)).abs();
    if a <= b && a <= c {
        left
    } else if b <= c {
        above
    } else {
        upper_left
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::DeflateEncoder, Compression};

    fn chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        output.extend_from_slice(&(data.len() as u32).to_be_bytes());
        output.extend_from_slice(kind);
        output.extend_from_slice(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        output.extend_from_slice(&crc.finalize().to_be_bytes());
    }
    fn fixture(width: u32, height: u32, alpha: bool, interlaced: bool) -> Vec<u8> {
        let mut output = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut output, b"CgBI", &[0x40, 0, 0, 0]);
        let mut header = width.to_be_bytes().to_vec();
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, if alpha { 6 } else { 2 }, 0, 0, interlaced as u8]);
        chunk(&mut output, b"IHDR", &header);
        let passes: &[(u32, u32, u32, u32)] = if interlaced {
            &[
                (0, 0, 8, 8),
                (4, 0, 8, 8),
                (0, 4, 4, 8),
                (2, 0, 4, 4),
                (0, 2, 2, 4),
                (1, 0, 2, 2),
                (0, 1, 1, 2),
            ]
        } else {
            &[(0, 0, 1, 1)]
        };
        let mut raw = Vec::new();
        let channels = if alpha { 4 } else { 3 };
        for &(x0, y0, dx, dy) in passes {
            let mut previous = Vec::<u8>::new();
            for (row_number, y) in (y0..height).step_by(dy as usize).enumerate() {
                let mut row = Vec::new();
                for x in (x0..width).step_by(dx as usize) {
                    let divisor = if alpha { 3 } else { 1 };
                    row.extend_from_slice(&[
                        45 / divisor,
                        ((y % 4) * 15 + 30) as u8 / divisor,
                        ((x % 4) * 30 + 60) as u8 / divisor,
                    ]);
                    if alpha {
                        row.push(85);
                    }
                }
                if row.is_empty() {
                    continue;
                }
                let filter = (row_number % 5) as u8;
                raw.push(filter);
                if previous.is_empty() {
                    previous.resize(row.len(), 0);
                }
                for index in 0..row.len() {
                    let left = if index >= channels {
                        row[index - channels]
                    } else {
                        0
                    };
                    let up = previous[index];
                    let upper_left = if index >= channels {
                        previous[index - channels]
                    } else {
                        0
                    };
                    let predictor = match filter {
                        0 => 0,
                        1 => left,
                        2 => up,
                        3 => ((left as u16 + up as u16) / 2) as u8,
                        _ => paeth(left, up, upper_left),
                    };
                    raw.push(row[index].wrapping_sub(predictor));
                }
                previous = row;
            }
        }
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&raw).unwrap();
        chunk(&mut output, b"IDAT", &encoder.finish().unwrap());
        chunk(&mut output, b"IEND", &[]);
        output
    }
    #[test]
    fn decodes_rgb_rgba_all_filters_and_adam7_in_rust() {
        let directory = tempfile::tempdir().unwrap();
        for alpha in [false, true] {
            for interlaced in [false, true] {
                let path = directory.path().join("apple.png");
                fs::write(&path, fixture(17, 19, alpha, interlaced)).unwrap();
                let (_, source) =
                    Source::new(&directory.path().join("cache"), "apple", &path).unwrap();
                let image = decode(&source, &AtomicBool::new(false), 32 * 1024 * 1024)
                    .unwrap_or_else(|_| panic!("CgBI decode failed"))
                    .to_rgb8();
                assert_eq!(image.dimensions(), (17, 19));
                for (x, y, pixel) in image.enumerate_pixels() {
                    assert_eq!(
                        pixel.0,
                        [((x % 4) * 30 + 60) as u8, ((y % 4) * 15 + 30) as u8, 45]
                    );
                }
            }
        }
    }
    #[test]
    fn native_pipeline_preserves_exif_orientation_and_caches_cgbi_without_python() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("apple.png");
        let mut bytes = fixture(7, 9, true, true);
        let exif = [
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
            0,
        ];
        let mut exif_chunk = Vec::new();
        chunk(&mut exif_chunk, b"eXIf", &exif);
        bytes.splice(49..49, exif_chunk);
        fs::write(&path, bytes).unwrap();
        let (_, source) = Source::new(&directory.path().join("cache"), "apple", &path).unwrap();
        let decoded = decode(&source, &AtomicBool::new(false), 32 * 1024 * 1024)
            .unwrap_or_else(|_| panic!("CgBI EXIF decode failed"));
        assert_eq!((decoded.width(), decoded.height()), (9, 7));
        assert_eq!(decoded.to_rgb8().get_pixel(8, 0).0, [60, 30, 45]);
        let config = Config {
            workers: 2,
            memory_bytes: 64 * 1024 * 1024,
            python_fallback: false,
        };
        let result = super::super::generate(
            &source,
            &AtomicBool::new(false),
            &config,
            None,
            &RwLock::new(()),
        )
        .unwrap();
        assert_eq!(result.timings.backend, "native");
        let preview = image::open(&result.path).unwrap();
        assert!(preview.width() > preview.height());
        let result = super::super::generate(
            &source,
            &AtomicBool::new(false),
            &config,
            None,
            &RwLock::new(()),
        )
        .unwrap();
        assert!(result.timings.cache_hit);
    }
    #[test]
    fn rejects_corruption_truncation_cancellation_and_oversized_headers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("apple.png");
        let mut bytes = fixture(7, 9, true, true);
        bytes[30] ^= 1;
        fs::write(&path, &bytes).unwrap();
        let (_, source) = Source::new(&directory.path().join("cache"), "apple", &path).unwrap();
        assert!(decode(&source, &AtomicBool::new(false), 32 * 1024 * 1024).is_err());
        let bytes = fixture(7, 9, true, false);
        fs::write(&path, &bytes[..bytes.len() - 8]).unwrap();
        let (_, source) = Source::new(&directory.path().join("cache"), "apple", &path).unwrap();
        assert!(decode(&source, &AtomicBool::new(false), 32 * 1024 * 1024).is_err());
        fs::write(&path, &bytes).unwrap();
        let (_, source) = Source::new(&directory.path().join("cache"), "apple", &path).unwrap();
        assert!(decode(&source, &AtomicBool::new(true), 32 * 1024 * 1024).is_err());
        assert!(matches!(
            decode(&source, &AtomicBool::new(false), 1),
            Err(NativeError::Budget)
        ));
    }
}
