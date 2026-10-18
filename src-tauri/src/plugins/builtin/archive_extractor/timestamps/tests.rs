use super::super::execute_with_progress;
use super::*;
use std::io::Write;
use zip::{write::SimpleFileOptions, ZipWriter};

const MTIME_2017: u64 = 1_483_315_200;

#[test]
fn zip_restores_2017_file_and_directory_times_without_changing_source() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("old.zip");
    let mut writer = ZipWriter::new(fs::File::create(&input).unwrap());
    let options = SimpleFileOptions::default()
        .last_modified_time(zip::DateTime::from_date_and_time(2017, 1, 2, 12, 30, 40).unwrap());
    writer.add_directory("folder/", options).unwrap();
    for name in ["folder/one.txt", "folder/two.txt"] {
        writer.start_file(name, options).unwrap();
        writer.write_all(b"original contents").unwrap();
    }
    drop(writer.finish().unwrap());
    let original = fs::read(&input).unwrap();
    let source_mtime = fs::metadata(&input).unwrap().modified().unwrap();
    let output = root.path().join("output");
    let summary = execute_with_progress(
        vec![input.to_string_lossy().into_owned()],
        output.clone(),
        None,
        |_| {},
    )
    .unwrap();
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!(summary.extracted_files, 2);
    let expected: SystemTime = Local
        .with_ymd_and_hms(2017, 1, 2, 12, 30, 40)
        .earliest()
        .unwrap()
        .into();
    for name in ["folder", "folder/one.txt", "folder/two.txt"] {
        assert_eq!(
            fs::metadata(output.join("0000-old.zip").join(name))
                .unwrap()
                .modified()
                .unwrap(),
            expected
        );
    }
    assert_eq!(fs::read(&input).unwrap(), original);
    assert_eq!(
        fs::metadata(&input).unwrap().modified().unwrap(),
        source_mtime
    );
}

#[test]
fn tar_and_targz_restore_header_and_pax_times() {
    let root = tempfile::tempdir().unwrap();
    let mut builder = tar::Builder::new(Vec::new());
    let mut directory = tar::Header::new_gnu();
    directory.set_entry_type(tar::EntryType::Directory);
    directory.set_size(0);
    directory.set_mode(0o755);
    directory.set_mtime(MTIME_2017);
    directory.set_cksum();
    builder
        .append_data(&mut directory, "folder/", &[][..])
        .unwrap();
    for (name, pax) in [("folder/header.txt", false), ("folder/pax.txt", true)] {
        if pax {
            builder
                .append_pax_extensions([("mtime", b"1483315200.1234567".as_slice())])
                .unwrap();
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(4);
        header.set_mode(0o644);
        header.set_mtime(MTIME_2017);
        header.set_cksum();
        builder
            .append_data(&mut header, name, &b"data"[..])
            .unwrap();
    }
    let tar = builder.into_inner().unwrap();
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar).unwrap();
    let gzip = gzip.finish().unwrap();
    for (name, bytes) in [("old.tar", tar), ("old.tar.gz", gzip)] {
        let input = root.path().join(name);
        fs::write(&input, &bytes).unwrap();
        let source_mtime = fs::metadata(&input).unwrap().modified().unwrap();
        let output = root.path().join(format!("output-{name}"));
        let summary = execute_with_progress(
            vec![input.to_string_lossy().into_owned()],
            output.clone(),
            None,
            |_| {},
        )
        .unwrap();
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        assert_eq!(summary.extracted_files, 2);
        let output = output.join(format!("0000-{name}"));
        let expected = UNIX_EPOCH + Duration::from_secs(MTIME_2017);
        for name in ["folder", "folder/header.txt"] {
            assert_eq!(
                fs::metadata(output.join(name)).unwrap().modified().unwrap(),
                expected
            );
        }
        assert_eq!(
            fs::metadata(output.join("folder/pax.txt"))
                .unwrap()
                .modified()
                .unwrap(),
            expected + Duration::from_nanos(123_456_700)
        );
        assert_eq!(fs::read(&input).unwrap(), bytes);
        assert_eq!(
            fs::metadata(&input).unwrap().modified().unwrap(),
            source_mtime
        );
    }
}

#[test]
fn pax_timestamps_preserve_fractional_and_negative_values() {
    assert_eq!(
        parse_pax_time("-0.25"),
        UNIX_EPOCH.checked_sub(Duration::from_millis(250))
    );
    assert_eq!(
        parse_pax_time("1483315200.123456789"),
        UNIX_EPOCH.checked_add(Duration::new(MTIME_2017, 123_456_789))
    );
    assert_eq!(parse_pax_time("invalid"), None);
}

#[test]
fn zip_prefers_absolute_extra_field_times_over_dos_time() {
    for ntfs in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("absolute.zip");
        let mut writer = ZipWriter::new(fs::File::create(&input).unwrap());
        let mut options = zip::write::FullFileOptions::default()
            .last_modified_time(zip::DateTime::from_date_and_time(2026, 1, 1, 0, 0, 0).unwrap());
        let mut timestamp = vec![1]; // Extended timestamp flags: modification time present.
        timestamp.extend_from_slice(&(MTIME_2017 as u32).to_le_bytes());
        options.add_extra_data(0x5455, timestamp, false).unwrap();
        if ntfs {
            let ticks = (MTIME_2017 + 11_644_473_600) * 10_000_000 + 1_234_567;
            let mut field = vec![0; 4];
            field.extend_from_slice(&1u16.to_le_bytes());
            field.extend_from_slice(&24u16.to_le_bytes());
            for _ in 0..3 {
                field.extend_from_slice(&ticks.to_le_bytes());
            }
            options.add_extra_data(0x000a, field, false).unwrap();
        }
        writer.start_file("old.txt", options).unwrap();
        writer.write_all(b"data").unwrap();
        drop(writer.finish().unwrap());
        let output = root.path().join("output");
        let summary = execute_with_progress(
            vec![input.to_string_lossy().into_owned()],
            output.clone(),
            None,
            |_| {},
        )
        .unwrap();
        assert!(summary.errors.is_empty(), "{:?}", summary.errors);
        let expected = UNIX_EPOCH
            + Duration::from_secs(MTIME_2017)
            + if ntfs {
                Duration::from_nanos(123_456_700)
            } else {
                Duration::ZERO
            };
        assert_eq!(
            fs::metadata(output.join("0000-absolute.zip/old.txt"))
                .unwrap()
                .modified()
                .unwrap(),
            expected
        );
    }
}
