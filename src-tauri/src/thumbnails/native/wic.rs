//! Use installed Windows image codecs without launching a Python interpreter.
//! HEIF/AVIF codec availability varies by machine; keep the compatibility path.
use super::*;
use std::os::windows::ffi::OsStrExt;
use windows::{
    core::{w, HRESULT, PCWSTR},
    Win32::{
        Foundation::GENERIC_READ,
        Graphics::Imaging::*,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize,
            StructuredStorage::{PropVariantClear, PropVariantToUInt32, PROPVARIANT},
            CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
        },
    },
};

struct Apartment(bool);
impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

pub(super) fn decode(
    source: &Source,
    cancelled: &AtomicBool,
    budget: u64,
) -> Result<(DynamicImage, image::metadata::Orientation), NativeError> {
    check_cancel(cancelled)?;
    // COM interfaces stay on this worker and drop before its apartment guard.
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_MULTITHREADED);
        if initialized.is_err() && initialized != HRESULT(0x80010106u32 as i32) {
            return Err(NativeError::Unsupported);
        }
        let _apartment = Apartment(initialized.is_ok());
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(|_| NativeError::Unsupported)?;
        let path: Vec<u16> = source
            .path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(path.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(|_| NativeError::Unsupported)?;
        let frame = decoder.GetFrame(0).map_err(|_| NativeError::Unsupported)?;
        let (mut width, mut height) = (0, 0);
        frame
            .GetSize(&mut width, &mut height)
            .map_err(|_| NativeError::Unsupported)?;
        // Include the full decode and codec scratch even though CopyPixels only
        // requests a small result; codecs may internally retain original pixels.
        check_budget(
            width,
            height,
            u64::from(width) * u64::from(height) * 6,
            source.size,
            budget,
        )?;
        let mut orientation = image::metadata::Orientation::NoTransforms;
        if let Ok(metadata) = frame.GetMetadataQueryReader() {
            for query in [w!("/app1/ifd/{ushort=274}"), w!("/ifd/{ushort=274}")] {
                let mut value = PROPVARIANT::default();
                let result = metadata.GetMetadataByName(query, &mut value);
                let exif = if result.is_ok() {
                    PropVariantToUInt32(&value).ok()
                } else {
                    None
                };
                let _ = PropVariantClear(&mut value);
                if let Some(exif) = exif {
                    orientation =
                        image::metadata::Orientation::from_exif(exif as u8).unwrap_or(orientation);
                    break;
                }
            }
        }
        let longest = width.max(height);
        let target_width = ((u64::from(width) * EDGE as u64) / u64::from(longest)).max(1) as u32;
        let target_height = ((u64::from(height) * EDGE as u64) / u64::from(longest)).max(1) as u32;
        let scaler = factory
            .CreateBitmapScaler()
            .map_err(|_| NativeError::Unsupported)?;
        scaler
            .Initialize(
                &frame,
                target_width,
                target_height,
                WICBitmapInterpolationModeFant,
            )
            .map_err(|_| NativeError::Unsupported)?;
        let converter = factory
            .CreateFormatConverter()
            .map_err(|_| NativeError::Unsupported)?;
        converter
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat24bppRGB,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .map_err(|_| NativeError::Unsupported)?;
        check_cancel(cancelled)?;
        let mut pixels = vec![0; target_width as usize * target_height as usize * 3];
        converter
            .CopyPixels(std::ptr::null(), target_width * 3, &mut pixels)
            .map_err(|_| NativeError::Unsupported)?;
        check_cancel(cancelled)?;
        let image = image::RgbImage::from_raw(target_width, target_height, pixels)
            .ok_or_else(|| NativeError::Failed("Invalid Windows decoder pixels.".into()))?;
        Ok((DynamicImage::ImageRgb8(image), orientation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heic_and_avif_use_installed_codecs_or_report_compatibility_fallback() {
        let directory = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            (
                "image.heic",
                include_bytes!("../../../tests/fixtures/native-heic.heic").as_slice(),
            ),
            (
                "image.avif",
                include_bytes!("../../../tests/fixtures/native-avif.avif").as_slice(),
            ),
        ] {
            let path = directory.path().join(name);
            fs::write(&path, bytes).unwrap();
            let (_, source) = Source::new(&directory.path().join("cache"), name, &path).unwrap();
            match decode(&source, &AtomicBool::new(false), 64 * 1024 * 1024) {
                Ok((image, orientation)) => {
                    assert_eq!((image.width(), image.height()), (256, 128));
                    assert_eq!(orientation, image::metadata::Orientation::NoTransforms);
                    let pixel = image.to_rgb8().get_pixel(30, 30).0;
                    assert!(pixel[0] > 195 && pixel[1] < 50 && pixel[2] < 70);
                    eprintln!("{name}: native Windows codec available");
                }
                Err(NativeError::Unsupported) => {
                    eprintln!("{name}: Windows codec unavailable; compatibility fallback required")
                }
                Err(_) => panic!("Unexpected native codec error for {name}"),
            }
        }
    }
    #[test]
    fn windows_codec_path_scales_and_preserves_color_without_python() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        image::RgbImage::from_pixel(800, 400, image::Rgb([210, 30, 50]))
            .save(&path)
            .unwrap();
        let (_, source) = Source::new(&directory.path().join("cache"), "image", &path).unwrap();
        let (image, _) = decode(&source, &AtomicBool::new(false), 64 * 1024 * 1024)
            .unwrap_or_else(|_| panic!("Windows PNG codec failed"));
        assert_eq!((image.width(), image.height()), (256, 128));
        assert_eq!(image.to_rgb8().get_pixel(80, 80).0, [210, 30, 50]);
    }
}
