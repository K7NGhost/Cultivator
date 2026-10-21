use super::*;
use image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, ImageDecoder, ImageError,
    ImageReader, Limits,
};
use std::io::{BufReader, Read, Seek, Write};

mod jpeg;

fn check_cancel(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("Thumbnail request cancelled.".into())
    } else {
        Ok(())
    }
}

pub(super) fn generate(
    source: &Source,
    cancelled: &AtomicBool,
    config: &Config,
    fallback: Option<&Fallback>,
    decode_gate: &RwLock<()>,
) -> Outcome {
    let started = Instant::now();
    check_cancel(cancelled)?;
    if source.target.is_file() {
        return Ok(Thumbnail {
            path: source.target.to_string_lossy().into_owned(),
            timings: Timings {
                cache_hit: true,
                backend: "cache".into(),
                total_ms: millis(started),
                ..Timings::default()
            },
        });
    }
    let cache = source.target.parent().ok_or("Invalid thumbnail cache.")?;
    fs::create_dir_all(cache).map_err(|e| format!("Cannot create thumbnail cache: {e}"))?;
    let temporary = cache.join(format!(
        ".thumb-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let native_result = {
            let _guard = decode_gate
                .read()
                .map_err(|_| "Thumbnail decoder lock unavailable.")?;
            decode_native(source, cancelled, config.worker_budget())
        };
        let mut timings = match native_result {
            Ok((encoded, mut timings)) => {
                check_cancel(cancelled)?;
                let start = Instant::now();
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&temporary)
                    .map_err(|e| e.to_string())?;
                file.write_all(&encoded).map_err(|e| e.to_string())?;
                // Derived cache data does not require a disk-wide durability
                // barrier for every tile. Close before atomic publication.
                drop(file);
                timings.write_ms = millis(start);
                timings
            }
            Err(NativeError::Unsupported | NativeError::Budget) if config.python_fallback => {
                check_cancel(cancelled)?;
                let fallback =
                    fallback.ok_or("No optional Python thumbnail decoder configured.")?;
                // Compatibility codecs need full-resolution buffers. Run one at
                // a time, with native decoding paused, to use the entire budget.
                let _guard = decode_gate
                    .write()
                    .map_err(|_| "Thumbnail decoder lock unavailable.")?;
                check_cancel(cancelled)?;
                fallback(&source.path, &temporary, config.memory_bytes, cancelled)?
            }
            Err(NativeError::Unsupported) => {
                return Err("Unsupported native thumbnail format; Python fallback disabled.".into())
            }
            Err(NativeError::Budget) => {
                return Err("Image exceeds thumbnail decoded memory budget.".into())
            }
            Err(NativeError::Failed(error)) => return Err(error),
        };
        check_cancel(cancelled)?;
        if !source.unchanged() {
            return Err("Media source changed during thumbnail generation.".into());
        }
        let start = Instant::now();
        match fs::rename(&temporary, &source.target) {
            Ok(()) => {}
            Err(_) if source.target.is_file() => {
                fs::remove_file(&temporary).map_err(|e| e.to_string())?;
            }
            Err(error) => return Err(format!("Cannot publish thumbnail: {error}")),
        }
        timings.write_ms += millis(start);
        timings.total_ms = millis(started);
        Ok(Thumbnail {
            path: source.target.to_string_lossy().into_owned(),
            timings,
        })
    })();
    if result.is_err() {
        // Failed/cancelled jobs never publish partial thumbnails.
        if let Err(error) = fs::remove_file(&temporary) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("Thumbnail temporary-file cleanup failed: {error}");
            }
        }
    }
    result
}

enum NativeError {
    Unsupported,
    Budget,
    Failed(String),
}
impl From<ImageError> for NativeError {
    fn from(error: ImageError) -> Self {
        match error {
            ImageError::Unsupported(_) => Self::Unsupported,
            other => Self::Failed(other.to_string()),
        }
    }
}
impl From<std::io::Error> for NativeError {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error.to_string())
    }
}
impl From<String> for NativeError {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}

fn decode_native(
    source: &Source,
    cancelled: &AtomicBool,
    budget: u64,
) -> Result<(Vec<u8>, Timings), NativeError> {
    let start = Instant::now();
    // Bound compressed input too; header/metadata parsing is work before pixel allocation.
    if source.size > budget / 4 {
        return Err(NativeError::Failed(
            "Image exceeds thumbnail input memory budget.".into(),
        ));
    }
    let mut header = [0u8; 16];
    let mut file = fs::File::open(&source.path)?;
    let n = file.read(&mut header)?;
    if n == 16 && &header[12..16] == b"CgBI" {
        return Err(NativeError::Unsupported);
    }
    file.rewind()?;
    let mut reader = ImageReader::new(BufReader::new(file)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(budget / 4);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let is_jpeg = reader.format() == Some(image::ImageFormat::Jpeg);
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    // Reserve conservatively for decoder scratch, 16-bit RGBA pixels, orientation
    // copies, resampling and encoder buffers. Fixed worker slots bound aggregate work.
    let estimate = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(64)
        .saturating_add(source.size)
        .saturating_add(4 * 1024 * 1024);
    if estimate > budget && !is_jpeg {
        // Compatibility formats can safely use the full budget under the
        // exclusive gate when a single native slot is too small.
        return Err(NativeError::Budget);
    }
    let orientation = decoder.orientation()?;
    check_cancel(cancelled)?;
    let mut image = if is_jpeg {
        // Decode JPEGs at 1/2, 1/4 or 1/8 resolution before allocating pixels.
        // Camera photos otherwise fall into the serialized Python fallback even
        // though the gallery only needs a 256-pixel preview.
        match jpeg::decode(&source.path, source.size, budget) {
            Ok(image) => image,
            // Keep native color conversion for uncommon JPEG variants that
            // already fit. Larger compatibility images use the exclusive gate.
            Err(NativeError::Unsupported) if estimate <= budget => {
                DynamicImage::from_decoder(decoder)?
            }
            Err(error) => return Err(error),
        }
    } else {
        DynamicImage::from_decoder(decoder)?
    };
    let decode_ms = millis(start);
    check_cancel(cancelled)?;
    let start = Instant::now();
    image.apply_orientation(orientation);
    let thumbnail = image.resize(EDGE, EDGE, FilterType::Triangle).to_rgb8();
    let resize_ms = millis(start);
    drop(image);
    check_cancel(cancelled)?;
    let start = Instant::now();
    let mut encoded = Vec::new();
    JpegEncoder::new_with_quality(&mut encoded, 85).encode_image(&thumbnail)?;
    let encode_ms = millis(start);
    Ok((
        encoded,
        Timings {
            decode_ms,
            resize_ms,
            encode_ms,
            backend: "native".into(),
            ..Timings::default()
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_jpeg_stays_native_on_memory_saver_and_is_cached() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("photo.jpg");
        image::RgbImage::from_pixel(4608, 3456, image::Rgb([200, 40, 20]))
            .save(&path)
            .unwrap();
        let (_, source) = Source::new(&directory.path().join("cache"), "photo", &path).unwrap();
        let config = Config {
            workers: 8,
            memory_bytes: 256 * 1024 * 1024,
            python_fallback: false,
        };
        let result = generate(
            &source,
            &AtomicBool::new(false),
            &config,
            None,
            &RwLock::new(()),
        )
        .unwrap();
        assert_eq!(result.timings.backend, "native");
        let thumbnail = image::open(&result.path).unwrap().to_rgb8();
        assert_eq!(thumbnail.dimensions(), (256, 192));
        let pixel = thumbnail.get_pixel(100, 100);
        assert!(pixel[0] > 190 && pixel[1] < 50 && pixel[2] < 30);
        let cached = generate(
            &source,
            &AtomicBool::new(false),
            &config,
            None,
            &RwLock::new(()),
        )
        .unwrap();
        assert!(cached.timings.cache_hit);
    }

    #[test]
    fn compatibility_decoder_uses_full_budget_exclusively_and_caches_result() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("photo.heic");
        fs::write(&path, b"\0\0\0\x18ftypheic\0\0\0\0heicmif1").unwrap();
        let (_, source) = Source::new(&directory.path().join("cache"), "photo", &path).unwrap();
        let gate = Arc::new(RwLock::new(()));
        let fallback_gate = gate.clone();
        let config = Config {
            workers: 8,
            ..Config::default()
        };
        let full_budget = config.memory_bytes;
        let calls = Arc::new(AtomicU64::new(0));
        let fallback_calls = calls.clone();
        let fallback: Fallback = Arc::new(move |_, target, budget, _| {
            assert_eq!(budget, full_budget);
            assert!(fallback_gate.try_read().is_err());
            fallback_calls.fetch_add(1, Ordering::Relaxed);
            image::RgbImage::new(16, 16)
                .save_with_format(target, image::ImageFormat::Jpeg)
                .map_err(|error| error.to_string())?;
            Ok(Timings {
                backend: "python".into(),
                ..Timings::default()
            })
        });
        let cancelled = AtomicBool::new(false);
        let result = generate(&source, &cancelled, &config, Some(&fallback), &gate).unwrap();
        assert_eq!(result.timings.backend, "python");
        assert!(Path::new(&result.path).is_file());
        let cached = generate(&source, &cancelled, &config, Some(&fallback), &gate).unwrap();
        assert!(cached.timings.cache_hit);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
