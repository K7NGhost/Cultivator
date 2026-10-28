//! Versioned, independently verified compressed blocks of fixed-width telemetry.
use super::{format_time, ground_distance, valid_coordinates, FlightRow, FlightSummary};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub const VERSION: u32 = 1;
pub const NAME: &str = "telemetry.bin";
const MAGIC: &[u8; 8] = b"CDATBIN\0";
const HEADER: usize = 32;
const ROW: usize = 200;
pub(super) const BLOCK_ROWS: usize = 512;
const MAX_FOOTER: usize = 64 * 1024 * 1024;
const MAX_ROWS: usize = super::MAX_SOURCE_BYTES as usize / 12;
pub const COLUMNS: &[&str] = &[
    "id",
    "time",
    "elapsedSeconds",
    "tick",
    "latitude",
    "longitude",
    "coordinateSource",
    "coordinateTick",
    "coordinateOffset",
    "altitude",
    "altitudeReference",
    "barometricAltitude",
    "gpsAltitudeMsl",
    "controllerRelativeAltitude",
    "speed",
    "velocityN",
    "velocityE",
    "velocityD",
    "battery",
    "batteryVoltage",
    "batteryCurrent",
    "batteryTemperature",
    "mode",
    "satellites",
    "gpsUtc",
    "gpsFixType",
    "gpsHorizontalAccuracy",
    "gpsVerticalAccuracy",
    "sourceOffset",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheMetadata {
    pub version: u32,
    pub index_sha256: String,
}

#[derive(Serialize, Deserialize)]
struct Block {
    offset: u64,
    bytes: u32,
    rows: u32,
    sha256: String,
}

#[derive(Serialize, Deserialize)]
pub(super) struct Index {
    blocks: Vec<Block>,
    strings: Vec<String>,
    pub rows: usize,
    pub route: Vec<FlightRow>,
    pub samples: Vec<FlightRow>,
    pub summary: FlightSummary,
    pub route_points: usize,
    pub distance: Option<f64>,
}

struct Samples {
    rows: Vec<FlightRow>,
    seen: usize,
    stride: usize,
    limit: usize,
    last: Option<FlightRow>,
}
impl Samples {
    fn new(limit: usize) -> Self {
        Self {
            rows: Vec::new(),
            seen: 0,
            stride: 1,
            limit,
            last: None,
        }
    }
    fn push(&mut self, row: &FlightRow) {
        if self.seen % self.stride == 0 {
            self.rows.push(row.clone());
        }
        self.seen += 1;
        self.last = Some(row.clone());
        if self.rows.len() >= self.limit {
            let mut i = 0;
            self.rows.retain(|_| {
                let keep = i % 2 == 0;
                i += 1;
                keep
            });
            self.stride *= 2;
        }
    }
    fn finish(mut self) -> Vec<FlightRow> {
        if let Some(last) = self.last.take() {
            if self.rows.last().is_none_or(|r| r.id != last.id) {
                self.rows.push(last);
            }
        }
        self.rows
    }
}

pub(super) struct Writer {
    target: PathBuf,
    temporary: PathBuf,
    file: Option<File>,
    bytes: Vec<u8>,
    rows: usize,
    blocks: Vec<Block>,
    strings: Vec<String>,
    ids: HashMap<String, u32>,
    route: Samples,
    samples: Samples,
    extremes: [Option<FlightRow>; 4],
    previous_fix: Option<FlightRow>,
    summary: FlightSummary,
    route_points: usize,
    distance: f64,
}
impl Writer {
    pub fn new(path: &Path) -> Result<Self, String> {
        let temporary = path.with_file_name(format!("telemetry-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(err)?;
        file.write_all(&header(0)).map_err(err)?;
        Ok(Self {
            target: path.into(),
            temporary,
            file: Some(file),
            bytes: Vec::with_capacity(ROW * BLOCK_ROWS),
            rows: 0,
            blocks: Vec::new(),
            strings: Vec::new(),
            ids: HashMap::new(),
            route: Samples::new(20_000),
            samples: Samples::new(1024),
            extremes: [None, None, None, None],
            previous_fix: None,
            summary: FlightSummary::default(),
            route_points: 0,
            distance: 0.0,
        })
    }
    fn string(&mut self, text: Option<&str>) -> u32 {
        let Some(text) = text else {
            return 0;
        };
        if let Some(id) = self.ids.get(text) {
            return *id;
        }
        let id = self.strings.len() as u32 + 1;
        self.strings.push(text.into());
        self.ids.insert(text.into(), id);
        id
    }
    pub fn push(&mut self, row: FlightRow) -> Result<(), String> {
        if self.rows >= MAX_ROWS {
            return Err("Telemetry cache row limit exceeded".into());
        }
        let string_ids = [
            self.string(row.mode.as_deref()),
            self.string(row.gps_utc.as_deref()),
            self.string(row.coordinate_source.as_deref()),
            self.string(row.altitude_reference.as_deref()),
        ];
        encode(&row, string_ids, &mut self.bytes);
        self.rows += 1;
        self.summary.observe(&row);
        self.samples.push(&row);
        for (i, value) in [row.altitude, row.altitude, row.speed, row.speed]
            .into_iter()
            .enumerate()
        {
            if let Some(value) = value {
                let old =
                    self.extremes[i]
                        .as_ref()
                        .and_then(|r| if i < 2 { r.altitude } else { r.speed });
                if old.is_none_or(|old| if i % 2 == 0 { value < old } else { value > old }) {
                    self.extremes[i] = Some(row.clone());
                }
            }
        }
        if valid_coordinates(row.latitude, row.longitude)
            && self
                .previous_fix
                .as_ref()
                .is_none_or(|r| r.coordinate_tick != row.coordinate_tick)
        {
            if let Some(previous) = &self.previous_fix {
                self.distance += ground_distance(previous, &row);
            }
            self.route_points += 1;
            self.route.push(&row);
            self.previous_fix = Some(row);
        }
        if self.bytes.len() >= ROW * BLOCK_ROWS {
            self.flush_block()?;
        }
        Ok(())
    }
    fn flush_block(&mut self) -> Result<(), String> {
        if self.bytes.is_empty() {
            return Ok(());
        }
        let bytes = compress(&self.bytes)?;
        let file = self.file.as_mut().unwrap();
        let offset = file.stream_position().map_err(err)?;
        file.write_all(&bytes).map_err(err)?;
        self.blocks.push(Block {
            offset,
            bytes: bytes.len() as u32,
            rows: (self.bytes.len() / ROW) as u32,
            sha256: hash(&bytes),
        });
        self.bytes.clear();
        Ok(())
    }
    pub fn finish(mut self) -> Result<(Reader, CacheMetadata), String> {
        self.flush_block()?;
        let mut samples = std::mem::replace(&mut self.samples, Samples::new(1024)).finish();
        samples.extend(self.extremes.iter_mut().filter_map(Option::take));
        samples.sort_by_key(|r| r.id);
        samples.dedup_by_key(|r| r.id);
        let route = std::mem::replace(&mut self.route, Samples::new(20_000)).finish();
        let index = Index {
            blocks: std::mem::take(&mut self.blocks),
            strings: std::mem::take(&mut self.strings),
            rows: self.rows,
            route,
            samples,
            summary: self.summary,
            route_points: self.route_points,
            distance: (self.route_points > 0).then_some(self.distance),
        };
        let json = serde_json::to_vec(&index).map_err(err)?;
        if json.len() > MAX_FOOTER {
            return Err("Telemetry cache index limit exceeded".into());
        }
        let footer = compress(&json)?;
        let file = self.file.as_mut().unwrap();
        let offset = file.stream_position().map_err(err)?;
        let header = header(offset);
        file.write_all(&footer).map_err(err)?;
        file.seek(SeekFrom::Start(0)).map_err(err)?;
        file.write_all(&header).map_err(err)?;
        file.flush().map_err(err)?;
        drop(self.file.take());
        fs::rename(&self.temporary, &self.target).map_err(err)?;
        let metadata = CacheMetadata {
            version: VERSION,
            index_sha256: index_hash(&header, &footer),
        };
        Ok((
            Reader {
                path: self.target.clone(),
                index,
                decoded: Mutex::new(VecDeque::new()),
            },
            metadata,
        ))
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.temporary);
    }
}

pub(super) struct Reader {
    pub path: PathBuf,
    pub index: Index,
    decoded: Mutex<VecDeque<(usize, Arc<Vec<FlightRow>>)>>,
}
impl Reader {
    pub fn open(path: &Path, expected: &CacheMetadata, rows: usize) -> Result<Self, String> {
        let mut file = File::open(path).map_err(err)?;
        let len = file.metadata().map_err(err)?.len();
        let mut h = [0; HEADER];
        file.read_exact(&mut h).map_err(err)?;
        if &h[..8] != MAGIC
            || number32(&h[8..]) != VERSION
            || expected.version != VERSION
            || number32(&h[12..]) != ROW as u32
            || number32(&h[16..]) != BLOCK_ROWS as u32
        {
            return Err("Unsupported telemetry cache format/version".into());
        }
        let offset = number64(&h[24..]);
        if offset < HEADER as u64 || offset > len || len - offset > MAX_FOOTER as u64 {
            return Err("Invalid telemetry cache index bounds".into());
        }
        file.seek(SeekFrom::Start(offset)).map_err(err)?;
        let mut footer = vec![0; (len - offset) as usize];
        file.read_exact(&mut footer).map_err(err)?;
        if index_hash(&h, &footer) != expected.index_sha256 {
            return Err("Telemetry cache index hash mismatch".into());
        }
        let json = decompress(&footer, MAX_FOOTER)?;
        let index: Index = serde_json::from_slice(&json).map_err(err)?;
        if rows > MAX_ROWS
            || index.rows != rows
            || index.blocks.len() != rows.div_ceil(BLOCK_ROWS)
            || index.route.len() > 20_000
            || index.samples.len() > 1030
        {
            return Err("Invalid telemetry cache row/sample counts".into());
        }
        let mut next = HEADER as u64;
        let mut count = 0;
        for (i, block) in index.blocks.iter().enumerate() {
            if block.offset != next
                || block.bytes == 0
                || block.bytes as usize > ROW * BLOCK_ROWS + 1024
                || block.rows == 0
                || block.rows as usize > BLOCK_ROWS
                || (i + 1 < index.blocks.len() && block.rows as usize != BLOCK_ROWS)
            {
                return Err("Invalid telemetry cache block bounds".into());
            }
            next = next
                .checked_add(block.bytes as u64)
                .ok_or("Telemetry cache offset overflow")?;
            count += block.rows as usize;
        }
        if next != offset || count != rows {
            return Err("Telemetry cache index does not match its data".into());
        }
        Ok(Self {
            path: path.into(),
            index,
            decoded: Mutex::new(VecDeque::new()),
        })
    }
    pub fn block(&self, index: usize) -> Result<Arc<Vec<FlightRow>>, String> {
        let mut cached = self
            .decoded
            .lock()
            .map_err(|_| "Telemetry block cache unavailable")?;
        if let Some(i) = cached.iter().position(|(id, _)| *id == index) {
            let entry = cached.remove(i).unwrap();
            let rows = entry.1.clone();
            cached.push_front(entry);
            return Ok(rows);
        }
        drop(cached);
        let block = self
            .index
            .blocks
            .get(index)
            .ok_or("Invalid telemetry block index")?;
        let mut file = File::open(&self.path).map_err(err)?;
        file.seek(SeekFrom::Start(block.offset)).map_err(err)?;
        let mut bytes = vec![0; block.bytes as usize];
        file.read_exact(&mut bytes).map_err(err)?;
        if hash(&bytes) != block.sha256 {
            return Err("Telemetry cache block hash mismatch".into());
        }
        let data = decompress(&bytes, block.rows as usize * ROW)?;
        if data.len() != block.rows as usize * ROW {
            return Err("Truncated telemetry cache block".into());
        }
        let rows = Arc::new(
            data.chunks_exact(ROW)
                .map(|row| decode(row, &self.index.strings))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut cached = self
            .decoded
            .lock()
            .map_err(|_| "Telemetry block cache unavailable")?;
        cached.retain(|(id, _)| *id != index);
        cached.push_front((index, rows.clone()));
        cached.truncate(4);
        Ok(rows)
    }
    pub fn range(&self, start: usize, count: usize) -> Result<Vec<FlightRow>, String> {
        let end = start.saturating_add(count).min(self.index.rows);
        let mut rows = Vec::new();
        let mut cursor = start;
        while cursor < end {
            let block = self.block(cursor / BLOCK_ROWS)?;
            let n = (end - cursor).min(block.len() - cursor % BLOCK_ROWS);
            rows.extend_from_slice(&block[cursor % BLOCK_ROWS..cursor % BLOCK_ROWS + n]);
            cursor += n;
        }
        Ok(rows)
    }
    pub fn for_each(
        &self,
        mut visit: impl FnMut(&FlightRow) -> Result<(), String>,
    ) -> Result<(), String> {
        for i in 0..self.index.blocks.len() {
            let rows = self.block(i)?;
            for row in rows.iter() {
                visit(row)?;
            }
        }
        Ok(())
    }
}

fn header(offset: u64) -> [u8; HEADER] {
    let mut h = [0; HEADER];
    h[..8].copy_from_slice(MAGIC);
    h[8..12].copy_from_slice(&VERSION.to_le_bytes());
    h[12..16].copy_from_slice(&(ROW as u32).to_le_bytes());
    h[16..20].copy_from_slice(&(BLOCK_ROWS as u32).to_le_bytes());
    h[24..].copy_from_slice(&offset.to_le_bytes());
    h
}
fn index_hash(h: &[u8], footer: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(h);
    hash.update(footer);
    format!("{:x}", hash.finalize())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn compress(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(bytes).map_err(err)?;
    encoder.finish().map_err(err)
}
fn decompress(bytes: &[u8], max: usize) -> Result<Vec<u8>, String> {
    let mut result = Vec::new();
    ZlibDecoder::new(bytes)
        .take(max as u64 + 1)
        .read_to_end(&mut result)
        .map_err(err)?;
    if result.len() > max {
        return Err("Telemetry cache decompression limit exceeded".into());
    }
    Ok(result)
}
fn number64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes[..8].try_into().unwrap())
}
fn number32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

fn encode(row: &FlightRow, strings: [u32; 4], out: &mut Vec<u8>) {
    for value in [
        row.id as u64,
        row.elapsed_seconds.to_bits(),
        row.tick,
        row.coordinate_tick.unwrap_or_default(),
        row.coordinate_offset.unwrap_or_default() as u64,
        row.source_offset as u64,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    let mut flags = 0u32;
    for (i, value) in [
        row.latitude,
        row.longitude,
        row.altitude,
        row.barometric_altitude,
        row.gps_altitude_msl,
        row.controller_relative_altitude,
        row.speed,
        row.velocity_n,
        row.velocity_e,
        row.velocity_d,
        row.battery,
        row.battery_voltage,
        row.battery_current,
        row.battery_temperature,
        row.gps_horizontal_accuracy,
        row.gps_vertical_accuracy,
    ]
    .into_iter()
    .enumerate()
    {
        if value.is_some() {
            flags |= 1 << i;
        }
        out.extend_from_slice(&value.unwrap_or_default().to_le_bytes());
    }
    for (i, present) in [
        row.coordinate_tick.is_some(),
        row.coordinate_offset.is_some(),
        row.satellites.is_some(),
        row.gps_fix_type.is_some(),
    ]
    .into_iter()
    .enumerate()
    {
        if present {
            flags |= 1 << (16 + i);
        }
    }
    out.extend_from_slice(&flags.to_le_bytes());
    for id in strings {
        out.extend_from_slice(&id.to_le_bytes());
    }
    out.extend_from_slice(&row.satellites.unwrap_or_default().to_le_bytes());
    out.push(row.gps_fix_type.unwrap_or_default());
    out.push(0);
}
fn decode(b: &[u8], strings: &[String]) -> Result<FlightRow, String> {
    let flags = number32(&b[176..]);
    let f =
        |i: usize| ((flags & (1u32 << i)) != 0).then(|| f64::from_bits(number64(&b[48 + i * 8..])));
    let text = |i: usize| -> Result<Option<String>, String> {
        let id = number32(&b[180 + i * 4..]);
        if id == 0 {
            Ok(None)
        } else {
            strings
                .get(id as usize - 1)
                .cloned()
                .map(Some)
                .ok_or_else(|| "Invalid telemetry cache string index".into())
        }
    };
    let elapsed = f64::from_bits(number64(&b[8..]));
    Ok(FlightRow {
        id: usize::try_from(number64(b)).map_err(err)?,
        time: format_time(elapsed),
        elapsed_seconds: elapsed,
        tick: number64(&b[16..]),
        coordinate_tick: (flags & (1 << 16) != 0).then(|| number64(&b[24..])),
        coordinate_offset: if flags & (1 << 17) != 0 {
            Some(usize::try_from(number64(&b[32..])).map_err(err)?)
        } else {
            None
        },
        source_offset: usize::try_from(number64(&b[40..])).map_err(err)?,
        latitude: f(0),
        longitude: f(1),
        altitude: f(2),
        barometric_altitude: f(3),
        gps_altitude_msl: f(4),
        controller_relative_altitude: f(5),
        speed: f(6),
        velocity_n: f(7),
        velocity_e: f(8),
        velocity_d: f(9),
        battery: f(10),
        battery_voltage: f(11),
        battery_current: f(12),
        battery_temperature: f(13),
        gps_horizontal_accuracy: f(14),
        gps_vertical_accuracy: f(15),
        mode: text(0)?,
        gps_utc: text(1)?,
        coordinate_source: text(2)?,
        altitude_reference: text(3)?,
        satellites: (flags & (1 << 18) != 0)
            .then(|| u16::from_le_bytes(b[196..198].try_into().unwrap())),
        gps_fix_type: (flags & (1 << 19) != 0).then_some(b[198]),
    })
}

pub(super) fn values(row: &FlightRow) -> Vec<String> {
    fn f(value: Option<f64>) -> String {
        value
            .map(|v| ryu::Buffer::new().format(v).to_string())
            .unwrap_or_default()
    }
    fn n(value: Option<impl ToString>) -> String {
        value.map(|v| v.to_string()).unwrap_or_default()
    }
    vec![
        row.id.to_string(),
        row.time.clone(),
        f(Some(row.elapsed_seconds)),
        row.tick.to_string(),
        f(row.latitude),
        f(row.longitude),
        row.coordinate_source.clone().unwrap_or_default(),
        n(row.coordinate_tick),
        n(row.coordinate_offset),
        f(row.altitude),
        row.altitude_reference.clone().unwrap_or_default(),
        f(row.barometric_altitude),
        f(row.gps_altitude_msl),
        f(row.controller_relative_altitude),
        f(row.speed),
        f(row.velocity_n),
        f(row.velocity_e),
        f(row.velocity_d),
        f(row.battery),
        f(row.battery_voltage),
        f(row.battery_current),
        f(row.battery_temperature),
        row.mode.clone().unwrap_or_default(),
        n(row.satellites),
        row.gps_utc.clone().unwrap_or_default(),
        n(row.gps_fix_type),
        f(row.gps_horizontal_accuracy),
        f(row.gps_vertical_accuracy),
        row.source_offset.to_string(),
    ]
}
