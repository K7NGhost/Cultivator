//! Native DJI DAT -> CSV conversion, based on the local DatCon source.
mod packet;
mod schema;
mod telemetry;
#[cfg(test)]
pub(crate) use packet::fixture_packet;

use csv::{Position, StringRecord};
use packet::{Clock, Scanner};
use schema::parse_definitions;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use telemetry::{CoordinateSource, Telemetry};

const MAX_SOURCE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EVENTS: usize = 1_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlightRow {
    pub id: usize,
    pub time: String,
    pub elapsed_seconds: f64,
    pub tick: u64,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub coordinate_source: Option<String>,
    pub coordinate_tick: Option<u64>,
    pub coordinate_offset: Option<usize>,
    pub altitude: Option<f64>,
    pub altitude_reference: Option<String>,
    pub barometric_altitude: Option<f64>,
    pub gps_altitude_msl: Option<f64>,
    pub controller_relative_altitude: Option<f64>,
    pub speed: Option<f64>,
    pub velocity_n: Option<f64>,
    pub velocity_e: Option<f64>,
    pub velocity_d: Option<f64>,
    pub battery: Option<f64>,
    pub battery_voltage: Option<f64>,
    pub battery_current: Option<f64>,
    pub battery_temperature: Option<f64>,
    pub mode: Option<String>,
    pub satellites: Option<u16>,
    pub gps_utc: Option<String>,
    #[serde(default)]
    pub gps_fix_type: Option<u8>,
    #[serde(default)]
    pub gps_horizontal_accuracy: Option<f64>,
    #[serde(default)]
    pub gps_vertical_accuracy: Option<f64>,
    pub source_offset: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlightEvent {
    pub time: String,
    pub elapsed_seconds: f64,
    pub title: String,
    pub detail: String,
    pub severity: String,
    pub record: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordType {
    pub id: u16,
    pub payload_bytes: usize,
    pub count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(default)]
    pub decoder_version: u32,
    pub id: String,
    pub source_name: String,
    pub source_path: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub format: String,
    pub aircraft: String,
    pub firmware_date: String,
    pub csv_path: String,
    pub csv_sha256: String,
    pub row_count: usize,
    pub packet_count: usize,
    pub rejected_packets: usize,
    pub skipped_bytes: usize,
    pub definition_count: usize,
    pub coordinate_source: String,
    pub route_point_count: usize,
    pub route_distance_meters: Option<f64>,
    pub unmapped_record_types: Vec<RecordType>,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvPage {
    pub columns: Vec<String>,
    pub rows: Vec<CsvRow>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Serialize)]
pub struct CsvRow {
    pub values: Vec<String>,
    pub telemetry: FlightRow,
}

#[derive(Serialize)]
pub struct ImportResult {
    pub metadata: Metadata,
    pub page: CsvPage,
    pub route: Vec<FlightRow>,
    pub events: Vec<FlightEvent>,
}

pub struct Dataset {
    pub metadata: Metadata,
    pub columns: Vec<String>,
    positions: Vec<Position>,
    pub route: Vec<FlightRow>,
    pub events: Vec<FlightEvent>,
}

pub fn format_time(seconds: f64) -> String {
    let millis = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        millis / 3_600_000,
        (millis / 60_000) % 60,
        (millis / 1000) % 60,
        millis % 1000
    )
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn convert_file(source: &Path, output_root: &Path) -> Result<Dataset, String> {
    convert_file_checked(source, output_root, &|| false)
}

pub fn convert_file_checked(
    source: &Path,
    output_root: &Path,
    cancelled: &dyn Fn() -> bool,
) -> Result<Dataset, String> {
    checkpoint(cancelled)?;
    let source = source
        .canonicalize()
        .map_err(|e| format!("Cannot open DAT file: {e}"))?;
    if !source
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("dat"))
    {
        return Err("Choose a .DAT flight log.".into());
    }
    let info = fs::metadata(&source).map_err(|e| e.to_string())?;
    if !info.is_file() {
        return Err("The selected path is not a file.".into());
    }
    if info.len() > MAX_SOURCE_BYTES {
        return Err("This version supports DAT files up to 512 MiB.".into());
    }
    let data = fs::read(&source).map_err(|e| format!("Cannot read DAT file: {e}"))?;
    let (start, version, clock_rate, aircraft) = header(&data)?;
    let id = uuid::Uuid::new_v4().to_string();
    fs::create_dir_all(output_root)
        .map_err(|e| format!("Cannot create conversion storage: {e}"))?;
    let directory = output_root.join(&id);
    fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let result = convert(
        &data, &source, &directory, &id, start, version, clock_rate, aircraft, cancelled,
    );
    if result.is_err() {
        // Only this newly created conversion directory is removed on failure.
        let _ = fs::remove_dir_all(&directory);
    }
    result
}

fn header(data: &[u8]) -> Result<(usize, &'static str, f64, &'static str), String> {
    if data.starts_with(b"LOGH") {
        return Err(
            "Encrypted LOGH DAT files are not supported by this DatCon-based parser.".into(),
        );
    }
    if data.get(16..21) != Some(b"BUILD") || data.len() < 128 {
        return Err(
            "Not a supported DJI DAT file: the BUILD header is missing or truncated.".into(),
        );
    }
    let v3 = data.get(242..252) == Some(b"DJI_LOG_V3");
    let (clock, aircraft) = match data[0] {
        5 => (85_000_000.0, "Matrice 100"),
        6 => (600.0, "Phantom 3 / Inspire 1"),
        11 => (4_500_000.0, "Phantom 4"),
        14 | 23 => (4_500_000.0, "Matrice 600"),
        16 => (4_500_000.0, "Mavic Pro"),
        17 => (4_500_000.0, "Inspire 2"),
        18 => (4_500_000.0, "Phantom 4 Pro"),
        20 => (4_500_000.0, "S900"),
        21 => (4_500_000.0, "Spark"),
        24 => (4_500_000.0, "Mavic Air"),
        25 => (4_500_000.0, "Matrice 200"),
        27 => (4_500_000.0, "Phantom 4 Advanced"),
        _ => {
            return Err(format!(
                "Unsupported aircraft/clock identifier {}. Refusing to guess elapsed times.",
                data[0]
            ));
        }
    };
    Ok((
        if v3 { 256 } else { 128 },
        if v3 { "DJI DAT V3" } else { "DJI DAT V1" },
        clock,
        aircraft,
    ))
}

#[allow(clippy::too_many_arguments)]
fn convert(
    data: &[u8],
    source: &Path,
    directory: &Path,
    id: &str,
    start: usize,
    version: &str,
    clock_rate: f64,
    aircraft: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Dataset, String> {
    let mut definitions_text = String::new();
    let mut types: BTreeMap<(u16, usize), usize> = BTreeMap::new();
    let mut scanner = Scanner::new(data, start);
    let mut count = 0;
    let mut position_sources = [false; 3];
    for packet in scanner.by_ref() {
        if count % 4096 == 0 {
            checkpoint(cancelled)?;
        }
        count += 1;
        *types
            .entry((packet.id, packet.encrypted.len()))
            .or_default() += 1;
        if let Some(source) = matches!(packet.id, 1 | 5 | 12 | 2048 | 2096)
            .then(|| Telemetry::valid_position_source(packet.id, &packet.decode()))
            .flatten()
        {
            position_sources[match source {
                CoordinateSource::Gps => 0,
                CoordinateSource::Imu => 1,
                CoordinateSource::Osd => 2,
            }] = true;
        }
        if packet.id == 65533 {
            if definitions_text.len() > 8 * 1024 * 1024 {
                return Err("Embedded record definitions exceed the supported limit.".into());
            }
            let decoded = packet.decode();
            let end = decoded
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(decoded.len());
            definitions_text.push_str(&String::from_utf8_lossy(&decoded[..end]));
            definitions_text.push('\n');
        }
    }
    if count == 0 {
        return Err("No CRC-valid DJI records were found in the file.".into());
    }
    let definitions = parse_definitions(&definitions_text);
    fs::write(
        directory.join("record-definitions.json"),
        serde_json::to_vec_pretty(&definitions).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let csv_path = directory.join("telemetry.csv");
    let mut writer = csv::Writer::from_path(&csv_path).map_err(|e| e.to_string())?;
    let coordinate_source = [
        CoordinateSource::Gps,
        CoordinateSource::Imu,
        CoordinateSource::Osd,
    ]
    .into_iter()
    .enumerate()
    .find_map(|(index, source)| position_sources[index].then_some(source));
    let mut state = Telemetry::new(coordinate_source);
    let mut clock = Clock::default();
    let mut first_tick = None;
    let mut group_tick = None;
    let mut group_offset = start;
    let mut changed = false;
    let mut row_count = 0;
    let mut events = vec![];
    let mut events_omitted = 0;
    let mut out_of_order = 0;
    let mut mapped = std::collections::BTreeSet::new();
    for packet in Scanner::new(data, start) {
        if row_count % 4096 == 0 {
            checkpoint(cancelled)?;
        }
        let tick = clock.unwrap(packet.tick);
        let origin = *first_tick.get_or_insert(tick);
        if let Some(previous) = group_tick {
            if tick < previous {
                out_of_order += 1;
                continue;
            }
            if tick > previous {
                if changed {
                    row_count += 1;
                    writer
                        .serialize(state.row(row_count, previous, origin, clock_rate, group_offset))
                        .map_err(|e| e.to_string())?;
                }
                changed = false;
                group_tick = Some(tick);
                group_offset = packet.offset;
            }
        } else {
            group_tick = Some(tick);
            group_offset = packet.offset;
        }
        if packet.id == 65533 {
            mapped.insert((packet.id, packet.encrypted.len()));
            continue;
        }
        let old_mode = state.mode.clone();
        if state.decode(
            packet.id,
            &packet.decode(),
            definitions.get(&packet.id),
            tick,
            packet.offset,
        ) {
            changed = true;
            mapped.insert((packet.id, packet.encrypted.len()));
        }
        let mut message = None;
        if packet.id == 0x8000 {
            let bytes = packet.decode();
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            let text = String::from_utf8_lossy(&bytes[..end]).trim().to_string();
            if !text.is_empty() {
                message = Some(("Flight controller log".to_string(), text));
            }
            mapped.insert((packet.id, packet.encrypted.len()));
        } else if let Some(mode) = state.mode.as_ref().filter(|_| state.mode != old_mode) {
            message = Some((
                "Flight mode change".to_string(),
                format!("Observed controller mode: {mode}"),
            ));
        }
        if let Some((title, detail)) = message {
            if events.len() < MAX_EVENTS {
                let elapsed = tick.saturating_sub(origin) as f64 / clock_rate;
                // Raw log messages are not classified as warnings without a documented rule.
                events.push(FlightEvent {
                    time: format_time(elapsed),
                    elapsed_seconds: elapsed,
                    title,
                    detail,
                    severity: "info".into(),
                    record: row_count + 1,
                });
            } else {
                events_omitted += 1;
            }
        }
    }
    if changed {
        row_count += 1;
        writer
            .serialize(state.row(
                row_count,
                group_tick.unwrap(),
                first_tick.unwrap(),
                clock_rate,
                group_offset,
            ))
            .map_err(|e| e.to_string())?;
    }
    writer.flush().map_err(|e| e.to_string())?;
    if row_count == 0 {
        return Err("Valid packets were found, but none contain a supported telemetry layout. No sample values were substituted.".into());
    }
    for event in &mut events {
        event.record = event.record.min(row_count);
    }
    let unmapped_record_types: Vec<_> = types
        .into_iter()
        .filter(|(key, _)| !mapped.contains(key))
        .map(|((id, payload_bytes), count)| RecordType {
            id,
            payload_bytes,
            count,
        })
        .collect();
    let mut warnings = vec![];
    if scanner.rejected_packets > 0 || scanner.skipped_bytes > 0 {
        warnings.push(format!("Recovered around {} rejected packet candidates and {} skipped bytes. CRC-invalid records were excluded.", scanner.rejected_packets, scanner.skipped_bytes));
    }
    if out_of_order > 0 {
        warnings.push(format!(
            "Excluded {out_of_order} out-of-order packets from telemetry aggregation."
        ));
    }
    if !unmapped_record_types.is_empty() {
        warnings.push(format!("{} record layouts are outside this focused telemetry decoder. Their IDs, payload lengths, and counts are recorded in conversion.json. This is not a full DatCon export.", unmapped_record_types.len()));
    }
    if events_omitted > 0 {
        warnings.push(format!("The event preview is limited to {MAX_EVENTS} entries; {events_omitted} additional messages were omitted."));
    }
    let metadata = Metadata {
        decoder_version: 2,
        id: id.into(),
        source_name: source
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        source_path: source.to_string_lossy().into(),
        source_bytes: data.len() as u64,
        source_sha256: sha256(data),
        format: version.into(),
        aircraft: aircraft.into(),
        firmware_date: String::from_utf8_lossy(data.get(21..33).unwrap_or_default())
            .trim_matches(['\0', ' '])
            .into(),
        csv_path: csv_path.to_string_lossy().into(),
        csv_sha256: sha256(&fs::read(&csv_path).map_err(|e| e.to_string())?),
        row_count,
        packet_count: count,
        rejected_packets: scanner.rejected_packets,
        skipped_bytes: scanner.skipped_bytes,
        definition_count: definitions.len(),
        coordinate_source: coordinate_source
            .map_or("none", CoordinateSource::name)
            .into(),
        route_point_count: 0,
        route_distance_meters: None,
        unmapped_record_types,
        warnings,
    };
    let mut dataset = Dataset::load(metadata, events)?;
    if dataset.route.is_empty() {
        dataset
            .metadata
            .warnings
            .push("No valid GPS coordinates were found. The map will remain empty.".into());
    }
    fs::write(
        directory.join("conversion.json"),
        serde_json::to_vec_pretty(&dataset.metadata).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::write(
        directory.join("events.json"),
        serde_json::to_vec_pretty(&dataset.events).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(dataset)
}

fn checkpoint(cancelled: &dyn Fn() -> bool) -> Result<(), String> {
    if cancelled() {
        Err("Drone parsing cancelled.".into())
    } else {
        Ok(())
    }
}

impl Dataset {
    fn load(mut metadata: Metadata, events: Vec<FlightEvent>) -> Result<Self, String> {
        let mut reader = csv::Reader::from_path(&metadata.csv_path).map_err(|e| e.to_string())?;
        let columns: Vec<String> = reader
            .headers()
            .map_err(|e| e.to_string())?
            .iter()
            .map(String::from)
            .collect();
        let headers = reader.headers().map_err(|e| e.to_string())?.clone();
        let mut positions = vec![];
        let mut route = vec![];
        let mut last_coordinate_tick = None;
        let mut record = StringRecord::new();
        loop {
            let position = reader.position().clone();
            if !reader.read_record(&mut record).map_err(|e| e.to_string())? {
                break;
            }
            let row: FlightRow = record
                .deserialize(Some(&headers))
                .map_err(|e| format!("Cannot read generated CSV: {e}"))?;
            positions.push(position);
            if valid_coordinates(row.latitude, row.longitude)
                && row.coordinate_tick != last_coordinate_tick
            {
                last_coordinate_tick = row.coordinate_tick;
                route.push(row);
            }
        }
        metadata.route_point_count = route.len();
        metadata.route_distance_meters = (!route.is_empty()).then(|| {
            route
                .windows(2)
                .map(|pair| ground_distance(&pair[0], &pair[1]))
                .sum()
        });
        Ok(Self {
            metadata,
            columns,
            positions,
            route,
            events,
        })
    }

    pub fn page(
        &self,
        page: usize,
        page_size: usize,
        query: &str,
        mode: &str,
    ) -> Result<CsvPage, String> {
        if page_size == 0 || page_size > 500 {
            return Err("CSV page size must be between 1 and 500.".into());
        }
        let start = page.checked_mul(page_size).ok_or("Invalid page index")?;
        let mut reader =
            csv::Reader::from_path(&self.metadata.csv_path).map_err(|e| e.to_string())?;
        let headers = reader.headers().map_err(|e| e.to_string())?.clone();
        let query = query.trim().to_lowercase();
        let mut rows = vec![];
        let mut total = self.metadata.row_count;
        if query.is_empty() && mode.is_empty() {
            if let Some(position) = self.positions.get(start) {
                reader.seek(position.clone()).map_err(|e| e.to_string())?;
                for record in reader.records().take(page_size) {
                    let record = record.map_err(|e| e.to_string())?;
                    rows.push(CsvRow {
                        telemetry: record
                            .deserialize(Some(&headers))
                            .map_err(|e| e.to_string())?,
                        values: record.iter().map(String::from).collect(),
                    });
                }
            }
        } else {
            total = 0;
            for record in reader.records() {
                let record = record.map_err(|e| e.to_string())?;
                let row: FlightRow = record
                    .deserialize(Some(&headers))
                    .map_err(|e| e.to_string())?;
                if (!mode.is_empty() && row.mode.as_deref() != Some(mode))
                    || (!query.is_empty()
                        && !record
                            .iter()
                            .any(|value| value.to_lowercase().contains(&query)))
                {
                    continue;
                }
                if total >= start && rows.len() < page_size {
                    rows.push(CsvRow {
                        telemetry: row,
                        values: record.iter().map(String::from).collect(),
                    });
                }
                total += 1;
            }
        }
        Ok(CsvPage {
            columns: self.columns.clone(),
            rows,
            total,
            page,
            page_size,
        })
    }

    pub fn import_result(&self) -> Result<ImportResult, String> {
        Ok(ImportResult {
            metadata: self.metadata.clone(),
            page: self.page(0, 100, "", "")?,
            route: self.route.clone(),
            events: self.events.clone(),
        })
    }
    pub fn csv_path(&self) -> PathBuf {
        PathBuf::from(&self.metadata.csv_path)
    }
}

pub fn valid_coordinates(latitude: Option<f64>, longitude: Option<f64>) -> bool {
    matches!((latitude, longitude), (Some(lat), Some(lon)) if lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0 && (lat != 0.0 || lon != 0.0))
}

fn ground_distance(first: &FlightRow, last: &FlightRow) -> f64 {
    let latitude_a = first.latitude.unwrap().to_radians();
    let latitude_b = last.latitude.unwrap().to_radians();
    let delta_latitude = latitude_b - latitude_a;
    let delta_longitude = (last.longitude.unwrap() - first.longitude.unwrap()).to_radians();
    let a = (delta_latitude / 2.0).sin().powi(2)
        + latitude_a.cos() * latitude_b.cos() * (delta_longitude / 2.0).sin().powi(2);
    let a = a.clamp(0.0, 1.0);
    6_371_000.0 * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

/// Reopen a generated CSV without requiring or modifying its source DAT.
pub fn restore_file(root: &Path, import_id: &str) -> Result<Dataset, String> {
    let id = uuid::Uuid::parse_str(import_id).map_err(|_| "Invalid saved import ID")?;
    let directory = root.join(id.to_string());
    let mut metadata: Metadata = serde_json::from_slice(
        &fs::read(directory.join("conversion.json"))
            .map_err(|e| format!("Cannot reopen saved conversion: {e}"))?,
    )
    .map_err(|e| format!("Cannot read saved conversion metadata: {e}"))?;
    if metadata.decoder_version < 2 {
        return Err("This flight was converted with an older DAT decoder that accepted GPS data before acquisition. Run Drone Telemetry again to rebuild it with corrected GPS validation.".into());
    }
    if metadata.id != id.to_string() {
        return Err("Saved conversion ID does not match its directory.".into());
    }
    let csv_path = directory
        .join("telemetry.csv")
        .canonicalize()
        .map_err(|e| format!("Cannot reopen saved CSV: {e}"))?;
    if csv_path.parent()
        != Some(
            directory
                .canonicalize()
                .map_err(|e| e.to_string())?
                .as_path(),
        )
    {
        return Err("Saved CSV is outside its conversion directory.".into());
    }
    use std::io::Read;
    let mut file = fs::File::open(&csv_path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if format!("{:x}", hasher.finalize()) != metadata.csv_sha256 {
        return Err("Saved CSV hash does not match the original conversion. Re-import the DAT to create a new conversion.".into());
    }
    metadata.csv_path = csv_path.to_string_lossy().into();
    let events: Vec<FlightEvent> = serde_json::from_slice(
        &fs::read(directory.join("events.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Cannot read saved events: {e}"))?;
    let dataset = Dataset::load(metadata, events)?;
    if dataset.positions.len() != dataset.metadata.row_count {
        return Err("Saved CSV row count does not match its metadata.".into());
    }
    Ok(dataset)
}

#[cfg(test)]
mod tests;
