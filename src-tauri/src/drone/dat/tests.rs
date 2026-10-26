use super::packet::fixture_packet;
use super::*;

#[test]
fn cancellation_during_packet_scan_cleans_only_the_new_conversion() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let directory = tempfile_for_cancellation();
    let source = directory.join("cancel.DAT");
    let mut bytes = header_bytes(false);
    for tick in 0..5000 {
        bytes.extend(fixture_packet(1, tick, &imu(40.0, -73.0)));
    }
    fs::write(&source, &bytes).unwrap();
    let calls = AtomicUsize::new(0);
    let output = directory.join("conversions");
    let error = convert_file_checked(&source, &output, &|| {
        calls.fetch_add(1, Ordering::SeqCst) >= 2
    })
    .err()
    .unwrap();
    assert!(error.contains("cancelled"));
    assert_eq!(fs::read(&source).unwrap(), bytes);
    assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    fs::remove_dir_all(directory).unwrap();
}

fn tempfile_for_cancellation() -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("cultivator-drone-cancel-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&path).unwrap();
    path
}

fn header_bytes(v3: bool) -> Vec<u8> {
    let mut data = vec![0; if v3 { 256 } else { 128 }];
    data[0] = if v3 { 16 } else { 6 };
    data[16..21].copy_from_slice(b"BUILD");
    if v3 {
        data[242..252].copy_from_slice(b"DJI_LOG_V3");
    }
    data
}

fn imu(latitude: f64, longitude: f64) -> Vec<u8> {
    let mut payload = vec![0; 120];
    payload[0..8].copy_from_slice(&longitude.to_radians().to_le_bytes());
    payload[8..16].copy_from_slice(&latitude.to_radians().to_le_bytes());
    payload[44..48].copy_from_slice(&42.5f32.to_le_bytes());
    payload[76..80].copy_from_slice(&3f32.to_le_bytes());
    payload[80..84].copy_from_slice(&4f32.to_le_bytes());
    payload[116] = 18;
    payload
}

fn gps(latitude_e7: i32, longitude_e7: i32, altitude_mm: i32) -> Vec<u8> {
    let mut bytes = vec![0; 72];
    bytes[0..4].copy_from_slice(&20170826u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&170000u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&longitude_e7.to_le_bytes());
    bytes[12..16].copy_from_slice(&latitude_e7.to_le_bytes());
    bytes[16..20].copy_from_slice(&altitude_mm.to_le_bytes());
    bytes[20..24].copy_from_slice(&300f32.to_le_bytes());
    bytes[24..28].copy_from_slice(&400f32.to_le_bytes());
    bytes[40..44].copy_from_slice(&3f32.to_le_bytes());
    bytes[64..66].copy_from_slice(&12u16.to_le_bytes());
    bytes
}

#[test]
fn gps_acquisition_excludes_startup_coordinates_and_preserves_fix_loss_quality() {
    let mut data = header_bytes(true);
    let mut invalid = gps(443_291_004, -1_296_740_355, 13_752_200);
    invalid[40..44].copy_from_slice(&0f32.to_le_bytes());
    invalid[64..66].copy_from_slice(&0u16.to_le_bytes());
    data.extend(fixture_packet(2096, 4_500_000, &invalid));
    let mut imu_fallback = header_bytes(true);
    imu_fallback.extend(fixture_packet(2048, 4_500_000, &imu(39.92, -105.11)));
    imu_fallback.extend(fixture_packet(2096, 4_500_000, &invalid));
    with_fixture(&imu_fallback, |dataset| {
        assert_eq!(dataset.metadata.coordinate_source, "imu");
        assert_eq!(dataset.route.len(), 1);
        assert!((dataset.route[0].latitude.unwrap() - 39.92).abs() < 1e-9);
        assert_eq!(dataset.route[0].speed, Some(5.0));
    });
    let mut acquired = gps(399_201_716, -1_051_129_505, 1_692_357);
    acquired[20..24].copy_from_slice(&9f32.to_le_bytes());
    acquired[24..28].copy_from_slice(&(-8f32).to_le_bytes());
    acquired[28..32].copy_from_slice(&2f32.to_le_bytes());
    acquired[32..36].copy_from_slice(&244f32.to_le_bytes());
    acquired[36..40].copy_from_slice(&435f32.to_le_bytes());
    data.extend(fixture_packet(2096, 9_000_000, &acquired));
    let mut lost = acquired.clone();
    lost[40..44].copy_from_slice(&0f32.to_le_bytes());
    lost[64..66].copy_from_slice(&0u16.to_le_bytes());
    data.extend(fixture_packet(2096, 13_500_000, &lost));
    with_fixture(&data, |dataset| {
        let page = dataset.page(0, 100, "", "").unwrap();
        let startup = &page.rows[0].telemetry;
        assert_eq!(startup.latitude, None);
        assert_eq!(startup.altitude, None);
        assert_eq!(startup.speed, None);
        assert_eq!(startup.gps_fix_type, Some(0));
        assert_eq!(dataset.route.len(), 2);
        let fix = &dataset.route[0];
        assert_eq!(fix.altitude, Some(1692.357));
        assert!((fix.speed.unwrap() - 0.1204159458).abs() < 1e-9);
        assert_eq!(fix.gps_horizontal_accuracy, Some(2.44));
        assert_eq!(fix.gps_vertical_accuracy, Some(4.35));
        assert_eq!(dataset.route[1].gps_fix_type, Some(0));
        assert_eq!(dataset.route[1].gps_horizontal_accuracy, None);
        assert_eq!(dataset.metadata.route_distance_meters, Some(0.0));
    });
}

/// Optional evidence check: no private DAT/CSV files are required in the repository.
#[test]
fn real_dat_matches_reference_csv_when_supplied() {
    let (Some(source), Some(reference)) = (
        std::env::var_os("CULTIVATOR_TEST_DAT"),
        std::env::var_os("CULTIVATOR_TEST_TRACK_CSV"),
    ) else {
        return;
    };
    let output = tempfile::tempdir().unwrap();
    let source = PathBuf::from(source);
    let original = sha256(&fs::read(&source).unwrap());
    let dataset = convert_file(&source, output.path()).unwrap();
    let mut reference = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .from_path(reference)
        .unwrap();
    let headers = reference.headers().unwrap().clone();
    let expected: Vec<_> = reference.records().map(Result::unwrap).collect();
    assert_eq!(dataset.route.len(), expected.len());
    for (row, record) in dataset.route.iter().zip(&expected) {
        let value = |name: &str| {
            record[headers.iter().position(|h| h == name).unwrap()]
                .parse::<f64>()
                .unwrap()
        };
        for (actual, name, tolerance) in [
            (row.latitude, "latitude", 0.00000051),
            (row.longitude, "longitude", 0.00000051),
            (row.gps_altitude_msl, "abs_alt_m", 0.00051),
            (row.speed, "ground_speed_ms", 0.00051),
            (row.velocity_n, "vel_n_ms", 0.00051),
            (row.velocity_e, "vel_e_ms", 0.00051),
            (row.velocity_d, "vel_d_ms", 0.00051),
        ] {
            assert!(
                (actual.unwrap() - value(name)).abs() <= tolerance,
                "{name} mismatch at row {}: {:?} versus {}",
                row.id,
                actual,
                value(name)
            );
        }
        assert_eq!(row.satellites.unwrap() as f64, value("sats_used"));
        assert_eq!(row.gps_fix_type.unwrap() as f64, value("gps_fix_type"));
        for (actual, name) in [
            (row.gps_horizontal_accuracy, "h_acc_m"),
            (row.gps_vertical_accuracy, "v_acc_m"),
        ] {
            let text = &record[headers.iter().position(|h| h == name).unwrap()];
            if text.is_empty() {
                assert_eq!(actual, None);
            } else {
                assert!((actual.unwrap() - text.parse::<f64>().unwrap()).abs() <= 0.0051);
            }
        }
    }
    assert_eq!(sha256(&fs::read(&source).unwrap()), original);
    println!("Reference CSV matched {} receiver positions: coordinates, MSL altitude, horizontal speed, N/E/D velocity, satellites, fix type and accuracy. Distance: {:.3} m; source unchanged.", expected.len(), dataset.metadata.route_distance_meters.unwrap());
}

#[test]
fn preserves_every_gps_fix_without_mixing_position_sources() {
    let mut data = header_bytes(true);
    let mut expected = vec![];
    for index in 0..5000u32 {
        let tick = 4_500_000 + index * 45_000;
        // Both missing IMU fixes and conflicting valid IMU positions must not
        // overwrite or precede the file's selected GPS receiver track.
        let payload = if index % 2 == 0 {
            imu(0.0, 0.0)
        } else {
            imu(38.0, -106.0)
        };
        data.extend(fixture_packet(2048, tick, &payload));
        if index % 197 == 1 {
            let latitude = 399_200_000 + index as i32;
            let longitude = -1_051_130_000 + (index as i32 % 1000);
            let offset = data.len();
            data.extend(fixture_packet(
                2096,
                tick,
                &gps(latitude, longitude, 1_660_000),
            ));
            expected.push((
                latitude as f64 / 1e7,
                longitude as f64 / 1e7,
                tick as u64,
                offset,
            ));
        }
        if index % 100 == 99 {
            data.extend(fixture_packet(12, tick, &[0; 55]));
        }
    }
    with_fixture(&data, |dataset| {
        assert!(dataset.metadata.row_count > 2000);
        assert_eq!(dataset.metadata.coordinate_source, "gps");
        assert_eq!(dataset.route.len(), expected.len());
        assert_eq!(dataset.metadata.route_point_count, expected.len());
        for (row, (latitude, longitude, tick, offset)) in dataset.route.iter().zip(expected) {
            assert_eq!(row.latitude, Some(latitude));
            assert_eq!(row.longitude, Some(longitude));
            assert_eq!(row.coordinate_tick, Some(tick));
            assert_eq!(row.coordinate_offset, Some(offset));
            assert_eq!(row.coordinate_source.as_deref(), Some("gps"));
            assert_eq!(row.altitude, Some(1660.0));
            assert_eq!(row.altitude_reference.as_deref(), Some("gps_msl"));
            assert_eq!(row.speed, Some(5.0));
            assert_eq!(row.satellites, Some(12));
        }
        assert_eq!(
            dataset.page(0, 1, "", "").unwrap().rows[0]
                .telemetry
                .latitude,
            None
        );
        let last = dataset
            .page(dataset.metadata.row_count - 1, 1, "", "")
            .unwrap();
        assert_eq!(
            last.rows[0].telemetry.coordinate_source.as_deref(),
            Some("gps")
        );
        assert!(last.rows[0].telemetry.latitude.is_some());
        assert_eq!(last.rows[0].telemetry.satellites, Some(12));
    });
}

fn with_fixture(data: &[u8], check: impl FnOnce(Dataset)) {
    let directory =
        std::env::temp_dir().join(format!("cultivator-dat-test-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    let source = directory.join("source.DAT");
    fs::write(&source, data).unwrap();
    let result = convert_file(&source, &directory.join("output")).unwrap();
    assert_eq!(fs::read(&source).unwrap(), data);
    check(result);
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn v1_combines_same_tick_and_keeps_last_group() {
    let mut data = header_bytes(false);
    let mut battery = vec![0; 45];
    battery[11] = 87;
    data.extend(fixture_packet(17, 600, &battery));
    data.extend(fixture_packet(1, 600, &imu(37.8, -122.4)));
    data.extend(fixture_packet(1, 1200, &imu(37.81, -122.41)));
    with_fixture(&data, |dataset| {
        assert_eq!(dataset.metadata.row_count, 2);
        let page = dataset.page(0, 1, "", "").unwrap();
        assert_eq!(page.total, 2);
        let row = &page.rows[0].telemetry;
        assert!((row.latitude.unwrap() - 37.8).abs() < 1e-8);
        assert_eq!(row.battery, Some(87.0));
        assert_eq!(row.speed, Some(5.0));
        assert_eq!(row.altitude_reference.as_deref(), Some("barometric"));
        assert_eq!(row.mode, None);
        assert_eq!(row.gps_utc, None);
        assert_eq!(
            dataset.page(1, 1, "", "").unwrap().rows[0]
                .telemetry
                .elapsed_seconds,
            1.0
        );
        assert!(dataset.page(5, 1, "", "").unwrap().rows.is_empty());
    });
}

#[test]
fn v3_reads_embedded_definitions_crc_recovery_and_filters_csv() {
    let mut data = header_bytes(true);
    data.extend(fixture_packet(65533, 4_500_000, b"Op.[config]\nname battery_info\ntype 4000\nOp.uint16_t packVol 0\nOp.int32_t current 0\nOp.uint16_t soc 0\n"));
    let mut battery = vec![];
    battery.extend(12000u16.to_le_bytes());
    battery.extend((-2000i32).to_le_bytes());
    battery.extend(80u16.to_le_bytes());
    data.extend(fixture_packet(4000, 4_500_000, &battery));
    let mut bad = fixture_packet(2048, 4_500_001, &imu(1.0, 2.0));
    bad[11] ^= 7;
    data.extend(bad);
    data.extend(fixture_packet(2048, 9_000_000, &imu(38.0, -123.0)));
    with_fixture(&data, |dataset| {
        assert_eq!(dataset.metadata.definition_count, 1);
        assert!(dataset.metadata.rejected_packets >= 1);
        let page = dataset.page(0, 100, "38", "").unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.rows[0].telemetry.battery, Some(80.0));
        assert_eq!(page.rows[0].telemetry.battery_voltage, Some(12.0));
        assert_eq!(page.rows[0].telemetry.battery_current, Some(2.0));
        assert!(dataset.page(0, 100, "", "GPS").unwrap().rows.is_empty());
        assert!(dataset.page(0, 501, "", "").is_err());
    });
}

#[test]
fn rejects_encrypted_truncated_unknown_clock_and_random_files() {
    assert!(header(b"LOGHencrypted").unwrap_err().contains("Encrypted"));
    assert!(header(b"not a DAT file").is_err());
    let mut data = header_bytes(true);
    data[0] = 255;
    assert!(header(&data).unwrap_err().contains("clock"));
    for length in 0..128 {
        assert!(header(&vec![0; length]).is_err());
    }
}

#[test]
fn missing_gps_does_not_create_a_fictitious_route() {
    let mut data = header_bytes(false);
    data.extend(fixture_packet(1, 600, &imu(0.0, 0.0)));
    with_fixture(&data, |dataset| {
        assert!(dataset.route.is_empty());
        let row = &dataset.page(0, 100, "", "").unwrap().rows[0].telemetry;
        assert_eq!(row.latitude, None);
        assert_eq!(row.longitude, None);
    });
}

#[test]
fn real_dat_round_trip_when_sample_is_supplied() {
    let Some(source) = std::env::var_os("CULTIVATOR_TEST_DAT") else {
        return;
    };
    let directory = PathBuf::from(
        std::env::var_os("CULTIVATOR_TEST_OUTPUT")
            .unwrap_or_else(|| std::env::temp_dir().into_os_string()),
    );
    let source = PathBuf::from(source);
    let original_hash = sha256(&fs::read(&source).unwrap());
    let dataset = convert_file(&source, &directory).unwrap();
    assert_eq!(original_hash, sha256(&fs::read(&source).unwrap()));
    assert_eq!(dataset.metadata.source_sha256, original_hash);
    assert!(dataset.metadata.row_count > 100);
    assert!(!dataset.route.is_empty());
    if let Some(reference_path) = std::env::var_os("CULTIVATOR_REFERENCE_TRACK") {
        let reference: serde_json::Value =
            serde_json::from_slice(&fs::read(reference_path).unwrap()).unwrap();
        let expected = reference["gps"].as_array().unwrap();
        assert_eq!(
            dataset.route.len(),
            expected.len(),
            "Every raw GPS fix must survive CSV loading"
        );
        for (row, expected) in dataset.route.iter().zip(expected) {
            assert!((row.latitude.unwrap() - expected["lat"].as_f64().unwrap()).abs() < 1e-9);
            assert!((row.longitude.unwrap() - expected["lon"].as_f64().unwrap()).abs() < 1e-9);
            assert!(
                (row.gps_altitude_msl.unwrap() - expected["alt"].as_f64().unwrap()).abs() < 1e-9
            );
            assert_eq!(row.coordinate_tick, expected["tick"].as_u64());
            assert_eq!(
                row.coordinate_offset,
                expected["offset"].as_u64().map(|value| value as usize)
            );
        }
        assert!(
            (dataset.metadata.route_distance_meters.unwrap() - 254.59586304699624).abs() < 0.01
        );
        println!(
            "Reference matched: all {} GPS fixes and {:.3} m ground distance",
            expected.len(),
            dataset.metadata.route_distance_meters.unwrap()
        );
    }
    assert!(dataset.page(1, 100, "", "").unwrap().rows[0].telemetry.id > 100);
    let preview = dataset.import_result().unwrap();
    fs::write(
        directory.join("real-import.json"),
        serde_json::to_vec(&preview).unwrap(),
    )
    .unwrap();
    println!(
        "Real DAT: {} valid packets, {} CSV rows, {} route points; source SHA-256 unchanged.",
        dataset.metadata.packet_count,
        dataset.metadata.row_count,
        dataset.route.len()
    );
}

#[test]
fn saved_conversion_reopens_without_dat_and_rejects_changed_csv() {
    let mut data = header_bytes(false);
    data.extend(fixture_packet(1, 600, &imu(37.8, -122.4)));
    data.extend(fixture_packet(1, 1200, &imu(37.81, -122.41)));
    with_fixture(&data, |dataset| {
        let id = dataset.metadata.id.clone();
        let csv_path = dataset.csv_path();
        let root = csv_path.parent().unwrap().parent().unwrap();
        fs::remove_file(&dataset.metadata.source_path).unwrap();
        let restored = restore_file(root, &id).unwrap();
        assert_eq!(restored.metadata.csv_sha256, dataset.metadata.csv_sha256);
        assert_eq!(restored.route.len(), dataset.route.len());
        assert_eq!(restored.page(1, 1, "", "").unwrap().rows[0].telemetry.id, 2);
        assert_eq!(restored.route[0].latitude, dataset.route[0].latitude);
        assert!(restore_file(root, "../source.DAT").is_err());
        let metadata_path = csv_path.parent().unwrap().join("conversion.json");
        let metadata_bytes = fs::read(&metadata_path).unwrap();
        let mut legacy: serde_json::Value = serde_json::from_slice(&metadata_bytes).unwrap();
        legacy.as_object_mut().unwrap().remove("decoderVersion");
        fs::write(&metadata_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(restore_file(root, &id)
            .err()
            .unwrap()
            .contains("Run Drone Telemetry again"));
        fs::write(&metadata_path, metadata_bytes).unwrap();
        let original_csv = fs::read(&csv_path).unwrap();
        fs::write(&csv_path, [original_csv.as_slice(), b"changed"].concat()).unwrap();
        let error = restore_file(root, &id).err().unwrap();
        assert!(error.contains("hash does not match"));
        fs::remove_file(&csv_path).unwrap();
        assert!(restore_file(root, &id)
            .err()
            .unwrap()
            .contains("Cannot reopen saved CSV"));
    });
}
