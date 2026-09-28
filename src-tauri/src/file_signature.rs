//! Extension mismatch: a file whose content contradicts its name, such as
//! "holiday.jpg" that is really a zip, a classic way to hide data in plain
//! sight. Only binary signatures count; text has none worth trusting.

use std::{fs::File, io::Read, path::Path};

/// Enough for every signature `infer` checks except inside-archive ones, and
/// a zip-based document still reads as "zip", which its family accepts.
const HEADER_BYTES: usize = 512;

/// Extensions that name the same kind of content.
const FAMILIES: &[&[&str]] = &[
    &["jpg", "jpeg", "jpe", "jfif"],
    &["tif", "tiff"],
    &["heic", "heif", "hif", "avif"],
    &["ico", "cur"],
    &["aiff", "aif", "aifc"],
    &["wav", "wave"],
    &["mp3", "mpga"],
    &["mpg", "mpeg", "mpe", "m1v", "m2v", "vob"],
    &["mp4", "m4v", "m4a", "m4b", "m4p", "m4r", "mov", "qt", "3gp", "3g2", "f4v"],
    &["mkv", "mka", "mks", "mk3d", "webm"],
    &["ogg", "oga", "ogv", "ogx", "opus", "spx"],
    &["gz", "tgz", "gzip"],
    &["bz2", "tbz", "tbz2"],
    &["xz", "txz"],
    &[
        "zip", "docx", "docm", "dotx", "xlsx", "xlsm", "xltx", "pptx", "pptm", "potx", "odt",
        "ods", "odp", "odg", "epub", "jar", "war", "ear", "apk", "aab", "aar", "ipa", "xpi",
        "kmz", "whl", "nupkg", "vsix", "appx", "msix", "3mf", "xps", "oxps", "cbz",
    ],
    &[
        "doc", "dot", "xls", "xlt", "ppt", "pot", "pps", "msi", "msp", "msg", "pub", "vsd",
        "mpp", "db",
    ],
    &[
        "exe", "dll", "sys", "scr", "cpl", "ocx", "drv", "efi", "mui", "ax", "com", "acm",
        "tlb", "winmd",
    ],
    &["elf", "so", "o", "ko", "bin", "axf", "prx", "out"],
    &[
        "sqlite", "sqlite3", "db", "db3", "sqlitedb", "sdb", "s3db", "sl3", "storedata",
    ],
    &["ttf", "ttc"],
];

/// Extensions that don't claim any particular content, so nothing in them
/// is a mismatch.
const GENERIC: &[&str] = &[
    "bak", "bin", "blob", "cache", "dat", "data", "download", "old", "part", "raw", "temp",
    "tmp",
];

/// What the content of `path` looks like, when it contradicts the extension.
pub fn contradicted_extension(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if GENERIC.contains(&extension.as_str()) {
        return None;
    }

    let mut header = Vec::with_capacity(HEADER_BYTES);
    File::open(path)
        .ok()?
        .take(HEADER_BYTES as u64)
        .read_to_end(&mut header)
        .ok()?;
    let kind = infer::get(&header)?;
    if kind.matcher_type() == infer::MatcherType::Text {
        return None;
    }

    let detected = kind.extension();
    (!same_kind(detected, &extension)).then_some(detected)
}

fn same_kind(detected: &str, extension: &str) -> bool {
    detected.eq_ignore_ascii_case(extension)
        || FAMILIES
            .iter()
            .any(|family| family.contains(&detected) && family.contains(&extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00];
    const ZIP: &[u8] = b"PK\x03\x04\x14\x00\x00\x00\x08\x00";

    fn file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let folder = std::env::temp_dir().join(format!("cultivator-signature-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn flags_content_that_contradicts_the_extension() {
        assert_eq!(contradicted_extension(&file("holiday.jpg", ZIP)), Some("zip"));
        assert_eq!(contradicted_extension(&file("notes.txt", JPEG)), Some("jpg"));
    }

    #[test]
    fn accepts_matching_and_related_extensions() {
        assert_eq!(contradicted_extension(&file("photo.jpeg", JPEG)), None);
        assert_eq!(contradicted_extension(&file("report.docx", ZIP)), None);
        assert_eq!(contradicted_extension(&file("cache.dat", JPEG)), None);
        assert_eq!(contradicted_extension(&file("no-extension", JPEG)), None);
        assert_eq!(contradicted_extension(&file("readme.md", b"# plain text")), None);
    }
}
