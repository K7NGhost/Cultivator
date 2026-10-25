use super::*;
use image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, DynamicImage, ImageDecoder, ImageError,
    ImageReader, Limits,
};
use std::io::{BufReader, Read, Seek, Write};

mod cgbi;
#[cfg(windows)]
mod heif;
mod jpeg;
#[cfg(windows)]
mod wic;

#[cfg(windows)]
pub(crate) fn render_heif_with_runtime(
    executable: &Path,
    source_path: &Path,
    target: &Path,
    budget: u64,
    cancelled: &AtomicBool,
) -> Option<Result<Timings, String>> {
    let started = Instant::now();
    let mut file = fs::File::open(source_path).ok()?;
    let mut header = [0u8; 16];
    if file.read(&mut header).ok()? != 16 || &header[4..8] != b"ftyp" {
        return None;
    }
    let size = file.metadata().ok()?.len();
    let image = heif::decode(executable, source_path, size, budget, cancelled).ok()?;
    Some((|| {
        let (encoded, mut timings) = encode_preview(
            image,
            image::metadata::Orientation::NoTransforms,
            started,
            cancelled,
        )
        .map_err(|error| match error {
            NativeError::Failed(error) => error,
            _ => "Native HEIF preview failed.".into(),
        })?;
        check_cancel(cancelled)?;
        let write = Instant::now();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(|e| e.to_string())?;
        file.write_all(&encoded).map_err(|e| e.to_string())?;
        timings.write_ms = millis(write);
        timings.total_ms = millis(started);
        timings.backend = "native-libheif".into();
        Ok(timings)
    })())
}

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
        let mut native_result = {
            let _guard = decode_gate
                .read()
                .map_err(|_| "Thumbnail decoder lock unavailable.")?;
            decode_native(source, cancelled, config.worker_budget())
        };
        // A large image may exceed one worker's share while fitting safely in
        // the overall budget. Retry Rust exclusively before starting Python.
        let mut exclusive = None;
        if matches!(native_result, Err(NativeError::Budget)) {
            exclusive = Some(
                decode_gate
                    .write()
                    .map_err(|_| "Thumbnail decoder lock unavailable.")?,
            );
            check_cancel(cancelled)?;
            native_result = decode_native(source, cancelled, config.memory_bytes);
        }
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
                if exclusive.is_none() {
                    exclusive = Some(
                        decode_gate
                            .write()
                            .map_err(|_| "Thumbnail decoder lock unavailable.")?,
                    );
                }
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
        drop(exclusive);
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
            ImageError::Limits(_) => Self::Budget,
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
    let (image, orientation) = decode_pixels(source, cancelled, budget)?;
    encode_preview(image, orientation, start, cancelled)
}

/// Reuse the native decoder and EXIF handling without JPEG encoding, IPC, or disk writes.
pub(crate) fn analysis_pixels(
    path: &Path,
    runtime: Option<&Path>,
    cancelled: &AtomicBool,
    decode_gate: &RwLock<()>,
    worker_budget: u64,
    total_budget: u64,
) -> Result<image::RgbImage, String> {
    let local = crate::evidence::ensure_local(path, cancelled)?;
    let path = local.as_path();

    let (_, source) = Source::new(Path::new(""), "visual-analysis", path)?;
    let decode = |budget| {
        let mut result = decode_pixels(&source, cancelled, budget);
        #[cfg(windows)]
        if matches!(result, Err(NativeError::Unsupported)) {
            if let Some(executable) = runtime {
                match heif::decode(executable, &source.path, source.size, budget, cancelled) {
                    Ok(image) => result = Ok((image, image::metadata::Orientation::NoTransforms)),
                    Err(NativeError::Budget) => return Err(NativeError::Budget),
                    Err(_) => {}
                }
            }
        }
        #[cfg(not(windows))]
        let _ = runtime;
        let (mut image, orientation) = result?;
        check_cancel(cancelled)?;
        image.apply_orientation(orientation);
        let pixels = image.resize(EDGE, EDGE, FilterType::Triangle).to_rgb8();
        check_cancel(cancelled)?;
        // Keep the decode gate until the full-resolution buffers are released.
        // An oversized image owns the whole budget through resizing, too.
        Ok(pixels)
    };
    let mut result = {
        let _guard = decode_gate
            .read()
            .map_err(|_| "Image decoder lock unavailable")?;
        decode(worker_budget)
    };
    if matches!(result, Err(NativeError::Budget)) {
        let _guard = decode_gate
            .write()
            .map_err(|_| "Image decoder lock unavailable")?;
        check_cancel(cancelled)?;
        result = decode(total_budget);
    }
    let pixels = result.map_err(|e| match e {
        NativeError::Unsupported => "No native decoder is available for this image".to_string(),
        NativeError::Budget => "Image exceeds the visual-analysis memory budget".to_string(),
        NativeError::Failed(message) => message,
    })?;
    if !source.unchanged() {
        return Err("Image changed during visual analysis".into());
    }
    Ok(pixels)
}

fn decode_pixels(
    source: &Source,
    cancelled: &AtomicBool,
    budget: u64,
) -> Result<(DynamicImage, image::metadata::Orientation), NativeError> {
    // Bound compressed input too; header/metadata parsing is work before pixel allocation.
    if source.size > budget / 4 {
        return Err(NativeError::Budget);
    }
    let mut header = [0u8; 16];
    let mut file = fs::File::open(&source.path)?;
    let n = file.read(&mut header)?;
    if n == 16 && &header[12..16] == b"CgBI" {
        let image = cgbi::decode(source, cancelled, budget)?;
        return Ok((image, image::metadata::Orientation::NoTransforms));
    }
    #[cfg(windows)]
    if n == 16 && &header[4..8] == b"ftyp" {
        let (image, orientation) = wic::decode(source, cancelled, budget)?;
        return Ok((image, orientation));
    }
    file.rewind()?;
    let mut reader = ImageReader::new(BufReader::new(file)).with_guessed_format()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(budget);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let is_jpeg = reader.format() == Some(image::ImageFormat::Jpeg);
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    if !is_jpeg && check_budget(width, height, decoder.total_bytes(), source.size, budget).is_err()
    {
        // Compatibility formats can safely use the full budget under the
        // exclusive gate when a single native slot is too small.
        return Err(NativeError::Budget);
    }
    let orientation = decoder.orientation()?;
    check_cancel(cancelled)?;
    let image = if is_jpeg {
        // Decode JPEGs at 1/2, 1/4 or 1/8 resolution before allocating pixels.
        // Camera photos otherwise fall into the serialized Python fallback even
        // though the gallery only needs a 256-pixel preview.
        match jpeg::decode(&source.path, source.size, budget) {
            Ok(image) => image,
            // Keep native color conversion for uncommon JPEG variants that
            // already fit. Larger compatibility images use the exclusive gate.
            Err(NativeError::Unsupported) => {
                check_budget(width, height, decoder.total_bytes(), source.size, budget)?;
                DynamicImage::from_decoder(decoder)?
            }
            Err(error) => return Err(error),
        }
    } else {
        DynamicImage::from_decoder(decoder)?
    };
    Ok((image, orientation))
}

fn check_budget(
    width: u32,
    height: u32,
    pixels: u64,
    input: u64,
    budget: u64,
) -> Result<(), NativeError> {
    // Full-size pixels, decoder scratch/orientation copies, the float resize
    // intermediate, bounded input and encoder buffers all count toward a job.
    let estimate = pixels
        .saturating_mul(3)
        .saturating_add(
            u64::from(width.max(height))
                .saturating_mul(EDGE as u64)
                .saturating_mul(16),
        )
        .saturating_add(input.saturating_mul(3))
        .saturating_add(4 * 1024 * 1024);
    if width == 0 || height == 0 || width > 32768 || height > 32768 || estimate > budget {
        Err(NativeError::Budget)
    } else {
        Ok(())
    }
}

fn encode_preview(
    mut image: DynamicImage,
    orientation: image::metadata::Orientation,
    start: Instant,
    cancelled: &AtomicBool,
) -> Result<(Vec<u8>, Timings), NativeError> {
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
    fn large_png_retries_natively_with_the_shared_budget_without_python() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("large.png");
        image::RgbaImage::from_pixel(4096, 3072, image::Rgba([190, 50, 30, 255]))
            .save(&path)
            .unwrap();
        let (_, source) = Source::new(&directory.path().join("cache"), "large", &path).unwrap();
        let config = Config {
            workers: 8,
            memory_bytes: 256 * 1024 * 1024,
            python_fallback: true,
        };
        let fallback: Fallback =
            Arc::new(|_, _, _, _| panic!("Large supported PNG must not launch Python"));
        let result = generate(
            &source,
            &AtomicBool::new(false),
            &config,
            Some(&fallback),
            &RwLock::new(()),
        )
        .unwrap();
        assert_eq!(result.timings.backend, "native");
        let image = image::open(&result.path).unwrap().to_rgb8();
        assert_eq!(image.dimensions(), (256, 192));
        let pixel = image.get_pixel(80, 80);
        assert!(pixel[0] > 180 && pixel[1] < 60 && pixel[2] < 40);
    }

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
