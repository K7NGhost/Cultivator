use super::*;
use std::io::Write;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

fn write_zip(path: &Path, files: &[(String, Vec<u8>)]) {
    let mut writer = ZipWriter::new(fs::File::create(path).unwrap());
    for (name, contents) in files {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(contents).unwrap();
    }
    writer.finish().unwrap();
}

#[test]
fn parallel_zip_preserves_contents_counts_and_callback_thread() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.zip");
    let files = (0..128)
        .map(|i| (format!("folder/{i}.bin"), vec![i as u8; 4096]))
        .collect::<Vec<_>>();
    write_zip(&path, &files);
    let output = root.path().join("output");
    let mut errors = Vec::new();
    let caller = thread::current().id();
    let mut events = Vec::new();
    let record = extract_zip_archive(&path, &output, &mut errors, None, &|| false, |n| {
        assert_eq!(thread::current().id(), caller);
        events.push(n);
    })
    .unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(record.extracted_files, 128);
    assert_eq!(record.extracted_bytes, 128 * 4096);
    assert_eq!(record.entries, 128);
    assert_eq!(events.last(), Some(&128));
    assert!(events.windows(2).all(|pair| pair[0] <= pair[1]));
    for (name, contents) in files {
        assert_eq!(fs::read(output.join(name)).unwrap(), contents);
    }
}

#[test]
fn sanitized_collisions_keep_archive_order_and_unsafe_paths_are_skipped() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.zip");
    write_zip(
        &path,
        &[
            ("a?.txt".into(), b"first".to_vec()),
            ("a*.txt".into(), b"last".to_vec()),
            ("../outside.txt".into(), b"unsafe".to_vec()),
        ],
    );
    let output = root.path().join("output");
    let mut errors = Vec::new();
    let record = extract_zip_archive(&path, &output, &mut errors, None, &|| false, |_| {}).unwrap();
    assert_eq!(fs::read(output.join("a-.txt")).unwrap(), b"last");
    assert_eq!(record.extracted_files, 2);
    assert_eq!(record.skipped_entries, 1);
    assert_eq!(errors.len(), 1);
    assert!(!root.path().join("outside.txt").exists());
}

#[test]
fn parallel_zip_respects_selective_matchers() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.zip");
    write_zip(
        &path,
        &[
            ("one.db".into(), b"one".to_vec()),
            ("two.db".into(), b"two".to_vec()),
            ("ignored.txt".into(), b"skip".to_vec()),
        ],
    );
    let mut manifest = super::super::manifest();
    manifest.mode = crate::plugins::PythonPluginMode::PathGlob;
    manifest.path_glob = vec!["*.db".into()];
    manifest.archive_path_glob.clear();
    let matchers = super::super::build_archive_entry_matchers(vec![manifest]).unwrap();
    let output = root.path().join("output");
    let mut errors = Vec::new();
    let record = extract_zip_archive(
        &path,
        &output,
        &mut errors,
        Some(&matchers),
        &|| false,
        |_| {},
    )
    .unwrap();
    assert!(errors.is_empty());
    assert_eq!(record.entries, 3);
    assert_eq!(record.extracted_files, 2);
    assert_eq!(record.extracted_bytes, 6);
    assert!(!output.join("ignored.txt").exists());
}
