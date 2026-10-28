use super::*;
use crate::packet::fixture_packet;
use inodex::image::RawImage;
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
};

fn log(v3: bool) -> Vec<u8> {
    let mut bytes = vec![0; if v3 { 256 } else { 128 }];
    bytes[0] = if v3 { 16 } else { 6 };
    bytes[16..22].copy_from_slice(b"BUILD ");
    if v3 {
        bytes[242..252].copy_from_slice(b"DJI_LOG_V3");
    }
    for tick in 1..=5 {
        bytes.extend(fixture_packet(2048, tick, &[0; 120]));
    }
    bytes
}

#[test]
fn validates_headers_across_scan_blocks_and_recovers_both_dat_versions() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("disk.img");
    let first = log(true);
    let second = log(false);
    let offset = BLOCK - 32;
    let mut bytes = vec![0; offset];
    bytes.extend(&first);
    bytes.resize(bytes.len() + MAX_GAP * 2, 0);
    let second_offset = bytes.len();
    bytes.extend(&second);
    fs::write(&path, &bytes).unwrap();
    let mut image = RawImage::open(&path).unwrap();
    let candidates = scan(&mut image, &|| false, |_, _, _| {}).unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].offset, offset as u64);
    assert_eq!(candidates[1].offset, second_offset as u64);
    assert_eq!(candidates[0].format, "DJI DAT V3");
    assert_eq!(candidates[1].format, "DJI DAT V1");
    let recovered = recover(
        &mut image,
        &candidates[0],
        second_offset as u64,
        1_000_000,
        &|| false,
        |_| {},
    )
    .unwrap();
    assert_eq!(recovered.bytes, first);
    assert!(!recovered.stopped_at_size_limit);
    let mut padded = first.clone();
    padded.resize(padded.len() + 512, 0);
    assert_eq!(
        recovered.fingerprint,
        fingerprint(&padded, &|| false).unwrap()
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[test]
fn rejects_headers_without_valid_records_and_honors_cancellation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("disk.img");
    let mut bytes = log(true);
    for byte in &mut bytes[256..] {
        *byte = 0;
    }
    fs::write(&path, &bytes).unwrap();
    let mut image = RawImage::open(path).unwrap();
    assert!(scan(&mut image, &|| false, |_, _, _| {})
        .unwrap()
        .is_empty());
    let cancel = AtomicBool::new(false);
    assert!(
        scan(&mut image, &|| cancel.load(Ordering::Relaxed), |_, _, _| {
            cancel.store(true, Ordering::Relaxed)
        })
        .unwrap_err()
        .contains("cancelled")
    );
}

#[test]
fn fingerprint_compares_the_entire_record_stream_not_a_shared_prefix() {
    let first = log(true);
    let mut second = first.clone();
    second.extend(fixture_packet(2048, 6, &[0; 120]));
    assert_ne!(
        fingerprint(&first, &|| false).unwrap(),
        fingerprint(&second, &|| false).unwrap()
    );
}

#[test]
fn recovery_stops_at_the_requested_limit_and_labels_a_bounded_fragment() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("disk.img");
    let bytes = log(true);
    fs::write(&path, &bytes).unwrap();
    let mut image = RawImage::open(path).unwrap();
    let candidate = Candidate {
        offset: 0,
        format: "DJI DAT V3".into(),
    };
    let limit = 256 + 3 * 132;
    let recovered = recover(
        &mut image,
        &candidate,
        bytes.len() as u64,
        limit,
        &|| false,
        |_| {},
    )
    .unwrap();
    assert_eq!(recovered.bytes.len() as u64, limit);
    assert!(recovered.stopped_at_size_limit);
}

#[test]
fn real_image_signature_scan_when_supplied() {
    let Some(path) = std::env::var_os("CULTIVATOR_TEST_CARVE_IMAGE") else {
        return;
    };
    let mut image = inodex::image::Image::open(path).unwrap();
    let candidates = scan(&mut image, &|| false, |_, _, _| {}).unwrap();
    let v3 = candidates
        .iter()
        .filter(|candidate| candidate.format == "DJI DAT V3")
        .count();
    println!(
        "Validated DAT candidates: {} total, {v3} V3, {} V1",
        candidates.len(),
        candidates.len() - v3
    );
    if let Ok(expected) = std::env::var("CULTIVATOR_TEST_CARVE_V3_COUNT") {
        assert_eq!(v3, expected.parse::<usize>().unwrap());
    }
}
