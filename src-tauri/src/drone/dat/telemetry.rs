use super::schema::{read_number, Definition};
use super::{format_time, valid_coordinates, FlightRow};

#[derive(Default)]
pub struct Telemetry {
    coordinate_source: Option<CoordinateSource>,
    coordinate_tick: Option<u64>,
    coordinate_offset: Option<usize>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    relative_height: Option<f64>,
    barometric: Option<f64>,
    gps_msl: Option<f64>,
    velocity_n: Option<f64>,
    velocity_e: Option<f64>,
    velocity_d: Option<f64>,
    battery: Option<f64>,
    voltage: Option<f64>,
    current: Option<f64>,
    temperature: Option<f64>,
    pub mode: Option<String>,
    satellites: Option<u16>,
    gps_utc: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordinateSource {
    Gps,
    Imu,
    Osd,
}

impl CoordinateSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::Gps => "gps",
            Self::Imu => "imu",
            Self::Osd => "osd",
        }
    }
}

fn position_payload(id: u16, bytes: &[u8]) -> Option<(CoordinateSource, Option<f64>, Option<f64>)> {
    match (id, bytes.len()) {
        (5, 68) | (2096, 66 | 68 | 72) => Some((
            CoordinateSource::Gps,
            i32_at(bytes, 12).map(|v| v / 1e7),
            i32_at(bytes, 8).map(|v| v / 1e7),
        )),
        (1 | 2048, 120) => Some((
            CoordinateSource::Imu,
            f64_at(bytes, 8).map(f64::to_degrees),
            f64_at(bytes, 0).map(f64::to_degrees),
        )),
        (12, 50 | 52 | 53 | 55) => Some((
            CoordinateSource::Osd,
            f64_at(bytes, 8).map(f64::to_degrees),
            f64_at(bytes, 0).map(f64::to_degrees),
        )),
        _ => None,
    }
}

fn f32_at(bytes: &[u8], offset: usize) -> Option<f64> {
    let value = f32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as f64;
    value.is_finite().then_some(value)
}
fn f64_at(bytes: &[u8], offset: usize) -> Option<f64> {
    let value = f64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
    value.is_finite().then_some(value)
}
fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}
fn i16_at(bytes: &[u8], offset: usize) -> Option<f64> {
    Some(i16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?) as f64)
}
fn i32_at(bytes: &[u8], offset: usize) -> Option<f64> {
    Some(i32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as f64)
}
fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn mode_name(mode: u8) -> String {
    match mode {
        0 => "Manual",
        1 => "ATTI",
        2 => "ATTI course lock",
        3 => "ATTI hover",
        4 => "Hover",
        5 => "GPS brake",
        6 => "GPS",
        7 => "GPS course lock",
        8 => "GPS home lock",
        9 => "GPS hot point",
        10 => "Assisted takeoff",
        11 => "Auto takeoff",
        12 => "Auto landing",
        13 => "ATTI landing",
        14 => "Navigation",
        15 => "RTH",
        16 => "Click go",
        17 => "Joystick",
        23 => "ATTI limited",
        24 => "GPS limited",
        25 => "Follow mission",
        26 => "Tracking",
        27 => "Pointing",
        _ => return format!("Mode {mode}"),
    }
    .into()
}

impl Telemetry {
    pub fn new(coordinate_source: Option<CoordinateSource>) -> Self {
        Self {
            coordinate_source,
            ..Self::default()
        }
    }

    pub fn valid_position_source(id: u16, bytes: &[u8]) -> Option<CoordinateSource> {
        position_payload(id, bytes).and_then(|(source, latitude, longitude)| {
            valid_coordinates(latitude, longitude).then_some(source)
        })
    }

    fn coords(&mut self, latitude: Option<f64>, longitude: Option<f64>) {
        if valid_coordinates(latitude, longitude) {
            self.latitude = latitude;
            self.longitude = longitude;
        } else {
            self.latitude = None;
            self.longitude = None;
        }
    }

    pub fn decode(
        &mut self,
        id: u16,
        bytes: &[u8],
        definition: Option<&Definition>,
        tick: u64,
        offset: usize,
    ) -> bool {
        if let Some((_, latitude, longitude)) = position_payload(id, bytes)
            .filter(|(source, _, _)| self.coordinate_source == Some(*source))
        {
            self.coords(latitude, longitude);
            self.coordinate_tick = Some(tick);
            self.coordinate_offset = Some(offset);
        }
        // Exact record IDs and accepted payload sizes mirror DatCon's dictionaries.
        match (id, bytes.len()) {
            (1 | 2048, 120) => {
                self.barometric = f32_at(bytes, 44);
                if self.coordinate_source != Some(CoordinateSource::Gps) {
                    self.velocity_n = f32_at(bytes, 76);
                    self.velocity_e = f32_at(bytes, 80);
                    self.velocity_d = f32_at(bytes, 84);
                    self.satellites = Some(bytes[116] as u16);
                }
                return true;
            }
            (5, 68) | (2096, 66 | 68 | 72) => {
                self.gps_msl = i32_at(bytes, 16).map(|v| v / 1000.0);
                self.velocity_n = f32_at(bytes, 20).map(|v| v / 100.0);
                self.velocity_e = f32_at(bytes, 24).map(|v| v / 100.0);
                self.velocity_d = f32_at(bytes, 28).map(|v| v / 100.0);
                self.satellites = u16_at(bytes, 64);
                let date = u32_at(bytes, 0).unwrap_or(0);
                let time = u32_at(bytes, 4).unwrap_or(0);
                let (year, month, day) = (date / 10000, date / 100 % 100, date % 100);
                let (hour, minute, second) = (time / 10000, time / 100 % 100, time % 100);
                if (2000..=2100).contains(&year)
                    && (1..=12).contains(&month)
                    && (1..=31).contains(&day)
                    && hour < 24
                    && minute < 60
                    && second < 60
                {
                    self.gps_utc = Some(format!(
                        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
                    ));
                }
                return true;
            }
            (12, 50 | 52 | 53 | 55) => {
                self.relative_height = i16_at(bytes, 16).map(|v| v / 10.0);
                self.mode = Some(mode_name(bytes[30] & 0x7f));
                return true;
            }
            (17, 45) => {
                self.battery = (bytes[11] <= 100).then_some(bytes[11] as f64);
                self.voltage = i16_at(bytes, 6).map(|v| v / 1000.0);
                self.current = u16_at(bytes, 8).map(|v| -(v as f64 - 65536.0) / 1000.0);
                self.temperature = Some(bytes[12] as i8 as f64);
                return true;
            }
            (5000, 26) => {
                self.battery = (bytes[12] <= 100).then_some(bytes[12] as f64);
                self.voltage = i16_at(bytes, 0).map(|v| v / 1000.0);
                self.current = i32_at(bytes, 2).map(|v| -v / 1000.0);
                self.temperature = Some(bytes[10] as i8 as f64);
                return true;
            }
            (5000, 38) => {
                self.battery = (bytes[18] <= 100).then_some(bytes[18] as f64);
                self.voltage = i32_at(bytes, 0).map(|v| v / 1000.0);
                self.current = i32_at(bytes, 4).map(|v| -v / 1000.0);
                self.temperature = i16_at(bytes, 16).map(|v| v / 10.0);
                return true;
            }
            _ => {}
        }
        // Definitions allow documented battery layouts that aren't in the fixed dictionary.
        if let Some(def) = definition.filter(|def| {
            def.length == bytes.len() && def.name.eq_ignore_ascii_case("battery_info")
        }) {
            let number = |name: &str| {
                def.fields
                    .iter()
                    .find(|field| field.name.eq_ignore_ascii_case(name))
                    .and_then(|field| read_number(bytes, field))
            };
            if let Some(soc) = number("soc").filter(|v| (0.0..=100.0).contains(v)) {
                self.battery = Some(soc);
            }
            self.voltage = number("packVol").map(|v| v / 1000.0);
            self.current = number("current").map(|v| -v / 1000.0);
            // Raw temperature units vary; leave blank without a known layout.
            self.temperature = None;
            return true;
        }
        false
    }

    pub fn row(
        &self,
        id: usize,
        tick: u64,
        first_tick: u64,
        clock: f64,
        offset: usize,
    ) -> FlightRow {
        let elapsed_seconds = tick.saturating_sub(first_tick) as f64 / clock;
        let (altitude, reference) =
            if self.coordinate_source == Some(CoordinateSource::Gps) && self.gps_msl.is_some() {
                (self.gps_msl, Some("gps_msl"))
            } else if self.relative_height.is_some() {
                (self.relative_height, Some("relative_to_home"))
            } else if self.gps_msl.is_some() {
                (self.gps_msl, Some("gps_msl"))
            } else if self.barometric.is_some() {
                (self.barometric, Some("barometric"))
            } else {
                (None, None)
            };
        let speed = match (self.velocity_n, self.velocity_e, self.velocity_d) {
            (Some(n), Some(e), Some(d)) => {
                let value = (n * n + e * e + d * d).sqrt();
                value.is_finite().then_some(value)
            }
            _ => None,
        };
        FlightRow {
            id,
            time: format_time(elapsed_seconds),
            elapsed_seconds,
            tick,
            latitude: self.latitude,
            longitude: self.longitude,
            coordinate_source: self
                .coordinate_source
                .map(|source| source.name().to_string()),
            coordinate_tick: self.coordinate_tick,
            coordinate_offset: self.coordinate_offset,
            altitude,
            altitude_reference: reference.map(String::from),
            barometric_altitude: self.barometric,
            gps_altitude_msl: self.gps_msl,
            controller_relative_altitude: self.relative_height,
            speed,
            velocity_n: self.velocity_n,
            velocity_e: self.velocity_e,
            velocity_d: self.velocity_d,
            battery: self.battery,
            battery_voltage: self.voltage,
            battery_current: self.current,
            battery_temperature: self.temperature,
            mode: self.mode.clone(),
            satellites: self.satellites,
            gps_utc: self.gps_utc.clone(),
            source_offset: offset,
        }
    }
}
