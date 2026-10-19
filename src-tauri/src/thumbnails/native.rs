use super::*;
use image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, ImageDecoder, ImageError,
    ImageReader, Limits,
};
use std::io::{BufReader, Read, Seek, Write};

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
        let mut timings = match decode_native(source, cancelled, config.worker_budget()) {
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
            Err(NativeError::Unsupported) if config.python_fallback => {
                check_cancel(cancelled)?;
                let fallback =
                    fallback.ok_or("No optional Python thumbnail decoder configured.")?;
                fallback(&source.path, &temporary, config.worker_budget(), cancelled)?
            }
            Err(NativeError::Unsupported) => {
                return Err("Unsupported native thumbnail format; Python fallback disabled.".into())
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
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    // Reserve conservatively for decoder scratch, 16-bit RGBA pixels, orientation
    // copies, resampling and encoder buffers. Fixed worker slots bound aggregate work.
    let estimate = u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(64)
        .saturating_add(source.size)
        .saturating_add(4 * 1024 * 1024);
    if estimate > budget {
        return Err(NativeError::Failed(
            "Image exceeds thumbnail decoded memory budget.".into(),
        ));
    }
    let orientation = decoder.orientation()?;
    check_cancel(cancelled)?;
    let mut image = DynamicImage::from_decoder(decoder)?;
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
