use super::*;
use crate::plugins::builtin::{BuiltinPlugin, ImageMetadataPlugin};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Cursor, Write},
    sync::atomic::{AtomicUsize, Ordering},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

fn png(width: u32) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::new(width, 3))
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
    output.into_inner()
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    write_zip_with_method(path, entries, CompressionMethod::Stored);
}

fn write_zip_with_method(path: &Path, entries: &[(&str, &[u8])], method: CompressionMethod) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(
            *name,
            SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn scan(paths: Vec<String>, output: &Path) -> MediaGallery {
    let mut progress = |_| {};
    let mut context = BuiltinPluginContext::new(
        paths,
        Some(output.to_path_buf()),
        serde_json::json!({"hashMedia":true}),
        vec![],
        &mut progress,
        &|| false,
    );
    ImageMetadataPlugin.run(&mut context).unwrap();
    context.into_effects().media.unwrap()
}

#[test]
fn scans_zip_directly_extracting_only_media_and_retaining_archive_provenance() {
    let case = tempfile::tempdir().unwrap();
    let source = case.path().join("device.ZIP");
    let image = png(4);
    write_zip_with_method(
        &source,
        &[
            ("DCIM/photo.PNG", &image),
            (
                "Movies/clip.MP4",
                b"\x00\x00\x00\x18ftypisom\x00\x00\x00\x00isom",
            ),
            ("Library/thumbnail", &image),
            ("unrelated/notes.txt", b"leave inside ZIP"),
            ("../escape.png", &image),
        ],
        CompressionMethod::Deflated,
    );
    let original = Sha256::digest(fs::read(&source).unwrap());
    let output = case.path().join("artifacts/media/source");
    let gallery = scan(vec![source.to_string_lossy().into_owned()], &output);
    assert_eq!(
        (
            gallery.photos.len(),
            gallery.videos.len(),
            gallery.scanned_files
        ),
        (2, 1, 3)
    );
    for item in gallery.photos.iter().chain(gallery.videos.iter()) {
        assert!(Path::new(&item.media_path).starts_with(fs::canonicalize(&output).unwrap()));
        assert!(Path::new(&item.media_path).is_file());
        assert!(item
            .metadata
            .iter()
            .any(|m| m.label == "Archive path" && m.value == source.to_string_lossy()));
        assert!(item.metadata.iter().any(|m| m.label == "Archive member"));
        assert!(item.metadata.iter().any(|m| m.label == "SHA-256"));
    }
    assert!(gallery
        .photos
        .iter()
        .all(|item| item.width == Some(4) && item.height == Some(3)));
    let mut files = Vec::new();
    crate::evidence::visit_files(&output, |path| {
        files.push(path.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert_eq!(files.len(), 3, "unrelated members must never be extracted");
    assert!(!case.path().join("escape.png").exists());
    assert_eq!(Sha256::digest(fs::read(&source).unwrap()), original);
}

#[test]
fn detects_zip_by_content_and_keeps_previous_working_copies_on_rerun() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("backup-without-extension");
    let image = png(2);
    write_zip(&source, &[("photo.png", &image)]);
    let output = directory.path().join("media");
    let first = scan(vec![source.to_string_lossy().into_owned()], &output);
    let second = scan(vec![source.to_string_lossy().into_owned()], &output);
    assert_eq!(first.photos.len(), 1);
    assert_eq!(second.photos.len(), 1);
    assert!(Path::new(&first.photos[0].media_path).is_file());
    assert_ne!(first.photos[0].media_path, second.photos[0].media_path);
}

#[test]
fn finds_zips_in_directories_without_duplicate_or_colliding_media() {
    let input = tempfile::tempdir().unwrap();
    let case = tempfile::tempdir().unwrap();
    let first = input.path().join("one.zip");
    let second = input.path().join("two.zip");
    write_zip(&first, &[("same/photo.png", &png(2))]);
    write_zip(&second, &[("same/photo.png", &png(5))]);
    fs::write(input.path().join("direct.png"), png(7)).unwrap();
    let gallery = scan(
        vec![
            input.path().to_string_lossy().into_owned(),
            first.to_string_lossy().into_owned(),
        ],
        &case.path().join("media"),
    );
    let mut widths: Vec<_> = gallery
        .photos
        .iter()
        .map(|item| item.width.unwrap())
        .collect();
    widths.sort();
    assert_eq!(widths, [2, 5, 7]);
}

#[test]
fn zip_with_no_media_leaves_unrelated_files_compressed_and_directory_named_zip_still_scans() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("empty-media.zip");
    write_zip(&source, &[("notes.txt", b"do not extract")]);
    let output = directory.path().join("output");
    let gallery = scan(vec![source.to_string_lossy().into_owned()], &output);
    assert!(gallery.photos.is_empty() && gallery.videos.is_empty());
    assert!(!output.exists());
    let folder = directory.path().join("folder.zip");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("photo.png"), png(3)).unwrap();
    assert_eq!(
        scan(vec![folder.to_string_lossy().into_owned()], &output)
            .photos
            .len(),
        1
    );
}

#[test]
fn corrupt_media_and_cancellation_do_not_publish_a_gallery_or_damage_prior_output() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("corrupt.zip");
    let image = png(8);
    write_zip(&source, &[("photo.png", &image)]);
    let offset = ZipArchive::new(File::open(&source).unwrap())
        .unwrap()
        .by_index(0)
        .unwrap()
        .data_start()
        .unwrap();
    let mut damaged = fs::read(&source).unwrap();
    damaged[offset as usize + image.len() - 1] ^= 1;
    fs::write(&source, damaged).unwrap();
    let output = directory.path().join("media");
    fs::create_dir(&output).unwrap();
    fs::write(output.join("previous.png"), &image).unwrap();
    let mut progress = |_| {};
    let mut context = BuiltinPluginContext::new(
        vec![source.to_string_lossy().into_owned()],
        Some(output.clone()),
        serde_json::json!({}),
        vec![],
        &mut progress,
        &|| false,
    );
    let error = ImageMetadataPlugin.run(&mut context).unwrap_err();
    assert!(error.contains("photo.png"), "{error}");
    assert!(context.into_effects().media.is_none());
    assert_eq!(fs::read(output.join("previous.png")).unwrap(), image);
    let mut extracted = Vec::new();
    crate::evidence::visit_files(&output, |path| {
        extracted.push(path.to_path_buf());
        Ok(())
    })
    .unwrap();
    assert_eq!(
        extracted,
        [output.join("previous.png")],
        "failed members must be cleaned up"
    );

    write_zip(&source, &[("photo.png", &image)]);
    let checks = AtomicUsize::new(0);
    let cancelled = || checks.fetch_add(1, Ordering::Relaxed) > 1;
    let mut context = BuiltinPluginContext::new(
        vec![source.to_string_lossy().into_owned()],
        Some(output.clone()),
        serde_json::json!({}),
        vec![],
        &mut progress,
        &cancelled,
    );
    assert!(ImageMetadataPlugin
        .run(&mut context)
        .unwrap_err()
        .contains("cancelled"));
    assert!(context.into_effects().media.is_none());
    assert_eq!(fs::read(output.join("previous.png")).unwrap(), image);
}
