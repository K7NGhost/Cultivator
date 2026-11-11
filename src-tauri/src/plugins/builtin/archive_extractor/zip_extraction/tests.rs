use super::*;
use std::io::{Seek, SeekFrom, Write};
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

fn corrupt_stored_member(path: &Path, with_good_member: bool) {
    let mut writer = ZipWriter::new(fs::File::create(path).unwrap());
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    writer.start_file("bad.db", options).unwrap();
    writer.write_all(b"original database content").unwrap();
    if with_good_member {
        writer.start_file("good.db", options).unwrap();
        writer.write_all(b"good database content").unwrap();
    }
    writer.finish().unwrap();
    let mut archive = ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let start = archive.by_index(0).unwrap().data_start().unwrap();
    let mut file = fs::OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(start)).unwrap();
    file.write_all(b"X").unwrap();
}

#[test]
fn crc_failure_removes_incomplete_member_and_reports_archive_and_filename() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.zip");
    corrupt_stored_member(&path, true);
    let output = root.path().join("output");
    let mut errors = Vec::new();
    let record = extract_zip_archive(&path, &output, &mut errors, None, &|| false, |_| {}).unwrap();
    assert_eq!(record.extracted_files, 1);
    assert_eq!(record.skipped_entries, 1);
    assert!(!output.join("bad.db").exists());
    assert_eq!(
        fs::read(output.join("good.db")).unwrap(),
        b"good database content"
    );
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("input.zip"), "{errors:?}");
    assert!(errors[0].contains("bad.db"), "{errors:?}");
    assert!(errors[0].to_lowercase().contains("checksum"), "{errors:?}");
}

#[test]
fn decoder_panic_retains_its_reason_and_removes_partial_output() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.zip");
    write_zip(&path, &[("large.db".into(), vec![7; 128 * 1024])]);
    let mut archive = open_archive(&path).unwrap();
    let task = EntryTask {
        index: 0,
        name: "large.db".into(),
        destination: root.path().join("output/large.db"),
        directory: false,
        modified: None,
    };
    let error = extract_entry(&mut archive, &task, &|| {
        if fs::metadata(&task.destination).is_ok_and(|metadata| metadata.len() > 0) {
            panic!("decoder overflow regression");
        }
        false
    })
    .unwrap_err();
    assert!(
        error.contains("ZIP decoder panicked: decoder overflow regression"),
        "{error}"
    );
    assert!(error.contains("large.db"), "{error}");
    assert!(!task.destination.exists());
}

#[test]
fn entry_open_failure_preserves_a_preexisting_destination() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.zip");
    write_zip(&path, &[("file.db".into(), b"content".to_vec())]);
    let mut archive = open_archive(&path).unwrap();
    let task = EntryTask {
        index: 99,
        name: "missing.db".into(),
        destination: root.path().join("existing.db"),
        directory: false,
        modified: None,
    };
    fs::write(&task.destination, b"preserve").unwrap();
    assert!(extract_entry(&mut archive, &task, &|| false).is_err());
    assert_eq!(fs::read(&task.destination).unwrap(), b"preserve");
}

#[test]
fn extraction_with_no_successful_members_fails_the_job_with_the_original_reason() {
    use crate::plugins::builtin::{BuiltinPlugin, BuiltinPluginContext};
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.zip");
    corrupt_stored_member(&path, false);
    let mut progress = |_| {};
    let mut context = BuiltinPluginContext::new(
        vec![path.to_string_lossy().into_owned()],
        Some(root.path().join("output")),
        serde_json::json!({}),
        vec![],
        &mut progress,
        &|| false,
    );
    let error = crate::plugins::builtin::ArchiveExtractorPlugin
        .run(&mut context)
        .unwrap_err();
    assert!(error.contains("Archive extraction failed"), "{error}");
    assert!(error.contains("bad.db"), "{error}");
    assert!(error.to_lowercase().contains("checksum"), "{error}");
}

#[test]
#[ignore = "requires an original multi-gigabyte ZIP in CULTIVATOR_LARGE_ZIP_TEST_PATH"]
fn members_larger_than_four_gib_stream_to_completion_with_crc_validation() {
    let path = std::env::var("CULTIVATOR_LARGE_ZIP_TEST_PATH").unwrap();
    let mut archive = open_archive(Path::new(&path)).unwrap();
    let mut large_members = 0;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        let expected = file.size();
        let compressed = file.compressed_size();
        let copied = cancellation::copy(&mut file, &mut std::io::sink(), &|| false).unwrap();
        assert_eq!(copied, expected);
        large_members += usize::from(compressed > u64::from(u32::MAX));
    }
    assert!(
        large_members > 0,
        "fixture must exercise the 4 GiB compressed-input boundary"
    );
}
