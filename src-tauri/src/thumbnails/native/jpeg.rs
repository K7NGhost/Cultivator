//! Scaled JPEG decoding keeps ordinary camera photos out of the process fallback.
use super::NativeError;
use image::{DynamicImage, GrayImage, RgbImage};
use jpeg_decoder::{CodingProcess, Decoder, PixelFormat};
use std::{fs::File, io::BufReader, path::Path};

pub(super) fn decode(
    path: &Path,
    input_bytes: u64,
    budget: u64,
) -> Result<DynamicImage, NativeError> {
    let mut decoder = Decoder::new(BufReader::new(File::open(path)?));
    decoder.set_max_decoding_buffer_size((budget / 4) as usize);
    decoder.read_info().map_err(decode_error)?;
    let info = decoder
        .info()
        .ok_or_else(|| NativeError::Failed("JPEG dimensions unavailable.".into()))?;
    if !matches!(info.pixel_format, PixelFormat::RGB24 | PixelFormat::L8) {
        return Err(NativeError::Unsupported);
    }
    if info.width > 32768 || info.height > 32768 {
        return Err(NativeError::Budget);
    }
    // Lossless and high-bit-depth JPEGs need the compatibility path. Progressive
    // scans additionally retain full-size coefficient planes despite IDCT scaling.
    if info.coding_process == CodingProcess::Lossless {
        return Err(NativeError::Unsupported);
    }
    let coefficients = if info.coding_process == CodingProcess::DctProgressive {
        u64::from(info.width).div_ceil(32) * u64::from(info.height).div_ceil(32) * 32 * 32 * 8
    } else {
        0
    };
    let (width, height) = decoder
        .scale(super::EDGE as u16, super::EDGE as u16)
        .map_err(decode_error)?;
    // Cover decoder planes, output, orientation copies, resampling and buffers.
    let estimate =
        u64::from(width) * u64::from(height) * 64 + coefficients + input_bytes + 4 * 1024 * 1024;
    if estimate > budget {
        return Err(NativeError::Budget);
    }
    let pixels = decoder.decode().map_err(decode_error)?;
    match info.pixel_format {
        PixelFormat::RGB24 => RgbImage::from_raw(width.into(), height.into(), pixels)
            .map(DynamicImage::ImageRgb8)
            .ok_or_else(|| NativeError::Failed("Invalid JPEG pixel buffer.".into())),
        PixelFormat::L8 => GrayImage::from_raw(width.into(), height.into(), pixels)
            .map(DynamicImage::ImageLuma8)
            .ok_or_else(|| NativeError::Failed("Invalid grayscale JPEG pixel buffer.".into())),
        // Retain the existing color conversion for less common JPEG color spaces.
        _ => Err(NativeError::Unsupported),
    }
}

fn decode_error(error: jpeg_decoder::Error) -> NativeError {
    match error {
        jpeg_decoder::Error::Unsupported(_) => NativeError::Unsupported,
        other => NativeError::Failed(format!("JPEG thumbnail decode failed: {other}")),
    }
}
