//! Minimal bindings to libheif's stable C API, loaded from the managed media
//! runtime. This reuses its bundled HEVC decoder without Python processes.
//! ABI reference: https://github.com/strukturag/libheif/blob/v1.19.8/libheif/api/libheif/heif.h
use super::*;
use libloading::os::windows::Library;
use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

#[repr(C)]
struct Error {
    code: i32,
    subcode: i32,
    message: *const c_char,
}
type Pointer = *mut c_void;
type Release = unsafe extern "C" fn(Pointer);
struct Heif {
    _library: Library,
    alloc: unsafe extern "C" fn() -> Pointer,
    free: Release,
    read: unsafe extern "C" fn(Pointer, *const c_void, usize, *const c_void) -> Error,
    primary: unsafe extern "C" fn(Pointer, *mut Pointer) -> Error,
    release_handle: Release,
    handle_width: unsafe extern "C" fn(Pointer) -> i32,
    handle_height: unsafe extern "C" fn(Pointer) -> i32,
    decode: unsafe extern "C" fn(Pointer, *mut Pointer, i32, i32, *const c_void) -> Error,
    release_image: Release,
    image_width: unsafe extern "C" fn(Pointer, i32) -> i32,
    image_height: unsafe extern "C" fn(Pointer, i32) -> i32,
    plane: unsafe extern "C" fn(Pointer, i32, *mut i32) -> *const u8,
    max_size: unsafe extern "C" fn(Pointer, i32),
    max_threads: unsafe extern "C" fn(Pointer, i32),
}

struct Object {
    pointer: Pointer,
    release: Release,
}
impl Drop for Object {
    fn drop(&mut self) {
        unsafe {
            (self.release)(self.pointer);
        }
    }
}
fn checked(error: Error) -> Result<(), NativeError> {
    if error.code == 0 {
        return Ok(());
    }
    let message = if error.message.is_null() {
        "HEIF codec error".into()
    } else {
        unsafe { CStr::from_ptr(error.message) }
            .to_string_lossy()
            .into_owned()
    };
    Err(NativeError::Failed(message))
}

impl Heif {
    unsafe fn load(site: &Path) -> Result<Self, String> {
        let path = fs::read_dir(site)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("libheif-") && name.ends_with(".dll"))
            })
            .ok_or("Managed HEIF library unavailable.")?;
        // Search only the codec's own directory and normal system/application
        // directories. Never resolve decoder dependencies from an evidence path.
        let library = Library::load_with_flags(path, 0x100 | 0x1000).map_err(|e| e.to_string())?;
        macro_rules! symbol {
            ($name:literal) => {
                *library
                    .get(concat!($name, "\0").as_bytes())
                    .map_err(|e| e.to_string())?
            };
        }
        let init: unsafe extern "C" fn(*const c_void) -> Error = symbol!("heif_init");
        let result = Self {
            alloc: symbol!("heif_context_alloc"),
            free: symbol!("heif_context_free"),
            read: symbol!("heif_context_read_from_memory_without_copy"),
            primary: symbol!("heif_context_get_primary_image_handle"),
            release_handle: symbol!("heif_image_handle_release"),
            handle_width: symbol!("heif_image_handle_get_width"),
            handle_height: symbol!("heif_image_handle_get_height"),
            decode: symbol!("heif_decode_image"),
            release_image: symbol!("heif_image_release"),
            image_width: symbol!("heif_image_get_width"),
            image_height: symbol!("heif_image_get_height"),
            plane: symbol!("heif_image_get_plane_readonly"),
            max_size: symbol!("heif_context_set_maximum_image_size_limit"),
            max_threads: symbol!("heif_context_set_max_decoding_threads"),
            _library: library,
        };
        if init(std::ptr::null()).code != 0 {
            return Err("Could not initialize native HEIF codec.".into());
        }
        // The library and its single initialization live for the process lifetime.
        Ok(result)
    }
}

pub(super) fn decode(
    executable: &Path,
    path: &Path,
    input: u64,
    budget: u64,
    cancelled: &AtomicBool,
) -> Result<DynamicImage, NativeError> {
    static LIBRARY: OnceLock<Result<Heif, String>> = OnceLock::new();
    check_cancel(cancelled)?;
    if input > budget / 4 {
        return Err(NativeError::Budget);
    }
    let site = executable
        .parent()
        .and_then(Path::parent)
        .ok_or(NativeError::Unsupported)?
        .join("Lib")
        .join("site-packages");
    let api = LIBRARY
        .get_or_init(|| unsafe { Heif::load(&site) })
        .as_ref()
        .map_err(|_| NativeError::Unsupported)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(input.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != input {
        return Err("HEIF source changed during decoding.".to_string().into());
    }
    // Local guards are ordered: input outlives context, which outlives image and
    // handle. All opaque allocations are released on cancellation/errors too.
    unsafe {
        let context = (api.alloc)();
        if context.is_null() {
            return Err("Could not allocate HEIF context.".to_string().into());
        }
        let context = Object {
            pointer: context,
            release: api.free,
        };
        // Limit coded image area too: a tiny display crop must not hide an
        // enormous compressed tile that exceeds the configured decode budget.
        let area_limit = ((budget / 18) as f64).sqrt().clamp(1.0, 32768.0) as i32;
        (api.max_size)(context.pointer, area_limit);
        (api.max_threads)(context.pointer, 0);
        checked((api.read)(
            context.pointer,
            bytes.as_ptr().cast(),
            bytes.len(),
            std::ptr::null(),
        ))?;
        let mut handle: Pointer = std::ptr::null_mut();
        checked((api.primary)(context.pointer, &mut handle))?;
        if handle.is_null() {
            return Err("HEIF primary image missing.".to_string().into());
        }
        let handle = Object {
            pointer: handle,
            release: api.release_handle,
        };
        let width = u32::try_from((api.handle_width)(handle.pointer))
            .map_err(|_| NativeError::Unsupported)?;
        let height = u32::try_from((api.handle_height)(handle.pointer))
            .map_err(|_| NativeError::Unsupported)?;
        check_budget(
            width,
            height,
            u64::from(width) * u64::from(height) * 6,
            input,
            budget,
        )?;
        check_cancel(cancelled)?;
        let mut decoded: Pointer = std::ptr::null_mut();
        // RGB colorspace=1, interleaved 8-bit RGB chroma=10. Default decoding
        // options apply container crop/rotation/mirroring to the primary image.
        checked((api.decode)(
            handle.pointer,
            &mut decoded,
            1,
            10,
            std::ptr::null(),
        ))?;
        if decoded.is_null() {
            return Err("HEIF decoder returned no pixels.".to_string().into());
        }
        let decoded = Object {
            pointer: decoded,
            release: api.release_image,
        };
        check_cancel(cancelled)?;
        let width = u32::try_from((api.image_width)(decoded.pointer, 10))
            .map_err(|_| NativeError::Unsupported)?;
        let height = u32::try_from((api.image_height)(decoded.pointer, 10))
            .map_err(|_| NativeError::Unsupported)?;
        check_budget(
            width,
            height,
            u64::from(width) * u64::from(height) * 6,
            input,
            budget,
        )?;
        let mut stride = 0i32;
        let plane = (api.plane)(decoded.pointer, 10, &mut stride);
        let row = width as usize * 3;
        if plane.is_null() || stride < 0 || (stride as usize) < row {
            return Err("Invalid HEIF RGB plane.".to_string().into());
        }
        let mut pixels = vec![0u8; row * height as usize];
        for y in 0..height as usize {
            check_cancel(cancelled)?;
            pixels[y * row..(y + 1) * row].copy_from_slice(std::slice::from_raw_parts(
                plane.add(y * stride as usize),
                row,
            ));
        }
        let image = image::RgbImage::from_raw(width, height, pixels)
            .ok_or_else(|| NativeError::Failed("Invalid HEIF pixel dimensions.".into()))?;
        Ok(DynamicImage::ImageRgb8(image))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn managed_native_heif_decodes_primary_colors_and_orientation_without_python() {
        let root = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("com.k7nghost.cultivator/python/environments"));
        let executable = root
            .and_then(|root| fs::read_dir(root).ok())
            .and_then(|entries| {
                entries.filter_map(Result::ok).find_map(|entry| {
                    let marker = fs::read_to_string(entry.path().join(".cultivator-ready")).ok()?;
                    marker
                        .contains("media-preview")
                        .then(|| entry.path().join("Scripts/python.exe"))
                })
            });
        let Some(executable) = executable else {
            eprintln!("Managed HEIF runtime not installed; native codec integration not exercised");
            return;
        };
        let directory = tempfile::tempdir().unwrap();
        for (name, bytes, dimensions) in [
            (
                "primary.heic",
                include_bytes!("../../../tests/fixtures/native-heic.heic").as_slice(),
                (64, 32),
            ),
            (
                "rotated.heic",
                include_bytes!("../../../tests/fixtures/native-heic-rotated.heic").as_slice(),
                (32, 64),
            ),
        ] {
            let path = directory.path().join(name);
            fs::write(&path, bytes).unwrap();
            let image = decode(
                &executable,
                &path,
                bytes.len() as u64,
                64 * 1024 * 1024,
                &AtomicBool::new(false),
            )
            .unwrap_or_else(|_| panic!("Native HEIF decode failed for {name}"));
            assert_eq!((image.width(), image.height()), dimensions);
            let pixel = image.to_rgb8().get_pixel(8, 8).0;
            assert!(pixel[0] > 195 && pixel[1] < 50 && pixel[2] < 70);
            let target = directory.path().join(format!("{name}.jpg"));
            let result = super::super::render_heif_with_runtime(
                &executable,
                &path,
                &target,
                64 * 1024 * 1024,
                &AtomicBool::new(false),
            )
            .expect("Native runtime should handle this HEIC")
            .unwrap();
            assert_eq!(result.backend, "native-libheif");
            let preview = image::open(target).unwrap();
            assert!(preview.width() <= 256 && preview.height() <= 256);
            eprintln!(
                "{name}: native libheif decode {:.1}ms, resize {:.1}ms",
                result.decode_ms, result.resize_ms
            );
        }
    }
}
