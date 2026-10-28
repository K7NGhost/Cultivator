use super::*;
use std::io::{BufWriter, Read, Write};

impl Dataset {
    pub(super) fn from_cache(
        metadata: Metadata,
        events: Vec<FlightEvent>,
        mut cache: cache::Reader,
    ) -> Self {
        Self {
            metadata,
            columns: cache::COLUMNS.iter().map(|v| (*v).into()).collect(),
            route: std::mem::take(&mut cache.index.route),
            samples: std::mem::take(&mut cache.index.samples),
            summary: cache.index.summary,
            cache,
            events,
        }
    }

    pub fn save_metadata(&self) -> Result<(), String> {
        write_metadata(
            self.cache
                .path
                .parent()
                .ok_or("Missing telemetry cache directory")?,
            &self.metadata,
        )
    }

    // Read legacy CSVs only during a one-time migration. Normal browsing never
    // opens a CSV and subsequent opens need neither the CSV nor the source DAT.
    fn load(mut metadata: Metadata, events: Vec<FlightEvent>) -> Result<Self, String> {
        let path = Path::new(&metadata.csv_path)
            .parent()
            .ok_or("Missing legacy conversion directory")?
            .join(cache::NAME);
        let mut writer = cache::Writer::new(&path)?;
        let mut rows = 0;
        for row in csv::Reader::from_path(&metadata.csv_path)
            .map_err(|e| e.to_string())?
            .deserialize::<FlightRow>()
        {
            writer.push(row.map_err(|e| e.to_string())?)?;
            rows += 1;
        }
        if rows != metadata.row_count {
            return Err("Saved CSV row count does not match its metadata".into());
        }
        let (cache, cache_metadata) = writer.finish()?;
        metadata.telemetry_cache = Some(cache_metadata);
        metadata.route_point_count = cache.index.route_points;
        metadata.route_distance_meters = cache.index.distance;
        Ok(Self::from_cache(metadata, events, cache))
    }

    pub fn page(
        &self,
        page: usize,
        page_size: usize,
        query: &str,
        mode: &str,
    ) -> Result<TelemetryPage, String> {
        if page_size == 0 || page_size > 500 {
            return Err("Telemetry page size must be between 1 and 500".into());
        }
        let start = page.checked_mul(page_size).ok_or("Invalid page index")?;
        let query = query.trim().to_lowercase();
        let mut rows = Vec::new();
        let total = if query.is_empty() && mode.is_empty() {
            for telemetry in self.cache.range(start, page_size)? {
                rows.push(TelemetryRow {
                    values: cache::values(&telemetry),
                    telemetry,
                });
            }
            self.metadata.row_count
        } else {
            let mut matched = 0;
            self.cache.for_each(|row| {
                if !mode.is_empty() && row.mode.as_deref() != Some(mode) {
                    return Ok(());
                }
                let values = cache::values(row);
                if !query.is_empty() && !values.iter().any(|v| v.to_lowercase().contains(&query)) {
                    return Ok(());
                }
                if matched >= start && rows.len() < page_size {
                    rows.push(TelemetryRow {
                        values,
                        telemetry: row.clone(),
                    });
                }
                matched += 1;
                Ok(())
            })?;
            matched
        };
        Ok(TelemetryPage {
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
            samples: self.samples.clone(),
            events: self.events.clone(),
        })
    }
    pub fn csv_path(&self) -> PathBuf {
        PathBuf::from(&self.metadata.csv_path)
    }

    /// This is the only normal path that serializes telemetry to CSV.
    pub fn export_csv(&self, destination: &Path) -> Result<String, String> {
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent.canonicalize().map_err(|e| e.to_string())?;
        let destination = parent.join(destination.file_name().ok_or("Missing CSV filename")?);
        let resolved = destination
            .canonicalize()
            .unwrap_or_else(|_| destination.clone());
        let directory = self
            .cache
            .path
            .parent()
            .ok_or("Missing telemetry cache directory")?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        let evidence_matches = |path: &str| {
            resolved
                == Path::new(path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(path))
        };
        if evidence_matches(&self.metadata.source_path)
            || self
                .metadata
                .carving
                .as_ref()
                .is_some_and(|c| evidence_matches(&c.image_path))
            || (resolved.starts_with(&directory)
                && resolved.file_name().is_none_or(|n| n != "telemetry.csv"))
        {
            return Err(
                "Choose an export destination outside the source evidence and conversion files"
                    .into(),
            );
        }
        let temporary = parent.join(format!(".cultivator-export-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            let mut writer = csv::WriterBuilder::new()
                .buffer_capacity(4 * 1024 * 1024)
                .from_writer(HashWriter {
                    file: BufWriter::new(file),
                    hash: Sha256::new(),
                });
            self.cache
                .for_each(|row| writer.serialize(row).map_err(|e| e.to_string()))?;
            writer.flush().map_err(|e| e.to_string())?;
            let hash = format!("{:x}", writer.get_ref().hash.clone().finalize());
            drop(writer);
            fs::rename(&temporary, &destination).map_err(|e| e.to_string())?;
            let mut metadata = self.metadata.clone();
            metadata.csv_path = destination.to_string_lossy().into_owned();
            metadata.csv_sha256 = hash.clone();
            metadata.csv_pending = false;
            write_metadata(&directory, &metadata)?;
            Ok(hash)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

struct HashWriter {
    file: BufWriter<fs::File>,
    hash: Sha256,
}
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let n = self.file.write(bytes)?;
        self.hash.update(&bytes[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn write_metadata(directory: &Path, metadata: &Metadata) -> Result<(), String> {
    let temporary = directory.join(format!("conversion-{}.tmp", uuid::Uuid::new_v4()));
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(metadata).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(&temporary, directory.join("conversion.json")).map_err(|e| {
        let _ = fs::remove_file(&temporary);
        e.to_string()
    })
}

fn events(directory: &Path) -> Result<Vec<FlightEvent>, String> {
    let path = directory.join("events.json");
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 4 * 1024 * 1024 {
        return Err("Saved events limit exceeded".into());
    }
    let events: Vec<FlightEvent> =
        serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if events.len() > MAX_EVENTS {
        return Err("Saved event count limit exceeded".into());
    }
    Ok(events)
}

pub fn restore_file(root: &Path, import_id: &str) -> Result<Dataset, String> {
    // Migration can be requested concurrently by a first page and a flight open.
    static MIGRATION: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _migration = MIGRATION
        .lock()
        .map_err(|_| "Telemetry migration lock unavailable")?;
    let id = uuid::Uuid::parse_str(import_id).map_err(|_| "Invalid saved import ID")?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let directory = root
        .join(id.to_string())
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if directory.parent() != Some(root.as_path()) {
        return Err("Saved flight is outside its conversion root".into());
    }
    let mut metadata: Metadata = serde_json::from_slice(
        &fs::read(directory.join("conversion.json"))
            .map_err(|e| format!("Cannot reopen saved conversion: {e}"))?,
    )
    .map_err(|e| format!("Cannot read saved conversion metadata: {e}"))?;
    if metadata.decoder_version < 2 {
        return Err(
            "Run Drone Telemetry again to rebuild this flight with corrected GPS validation".into(),
        );
    }
    if metadata.id != id.to_string() {
        return Err("Saved conversion ID does not match its directory".into());
    }
    if let Some(expected) = &metadata.telemetry_cache {
        let path = directory
            .join(cache::NAME)
            .canonicalize()
            .map_err(|e| format!("Cannot reopen telemetry cache: {e}"))?;
        if path.parent() != Some(directory.as_path()) {
            return Err("Saved telemetry cache is outside its conversion directory".into());
        }
        let cache = cache::Reader::open(&path, expected, metadata.row_count)?;
        return Ok(Dataset::from_cache(metadata, events(&directory)?, cache));
    }
    // Old deferred conversions require a single DAT decode into binary storage.
    if metadata.csv_pending {
        let data = retained_dat(&metadata)?;
        if sha256(&data) != metadata.source_sha256 {
            return Err("Saved DAT hash does not match the original conversion".into());
        }
        let (start, version, clock_rate, aircraft) = header(&data)?;
        let mut dataset = convert(
            &data,
            Path::new(&metadata.source_path),
            &directory,
            &metadata.id,
            start,
            version,
            clock_rate,
            aircraft,
            &|| false,
            false,
        )?;
        dataset.metadata.carving = metadata.carving;
        for warning in metadata.warnings {
            if !dataset.metadata.warnings.contains(&warning) {
                dataset.metadata.warnings.push(warning);
            }
        }
        dataset.save_metadata()?;
        return Ok(dataset);
    }
    // Completed legacy CSVs migrate without requiring the original DAT/image.
    let path = directory
        .join("telemetry.csv")
        .canonicalize()
        .map_err(|e| format!("Cannot reopen saved CSV: {e}"))?;
    if path.parent() != Some(directory.as_path()) {
        return Err("Saved CSV is outside its conversion directory".into());
    }
    let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    if format!("{:x}", digest.finalize()) != metadata.csv_sha256 {
        return Err("Saved CSV hash does not match the original conversion".into());
    }
    metadata.csv_path = path.to_string_lossy().into_owned();
    let dataset = Dataset::load(metadata, events(&directory)?)?;
    dataset.save_metadata()?;
    Ok(dataset)
}

fn retained_dat(metadata: &Metadata) -> Result<Vec<u8>, String> {
    if let Some(origin) = metadata.carving.as_ref().filter(|o| o.stored_as_range) {
        use inodex::image::DiskImage;
        let mut image =
            inodex::image::Image::open(&origin.image_path).map_err(|e| e.to_string())?;
        if origin.recovered_bytes > MAX_SOURCE_BYTES
            || origin
                .image_offset
                .checked_add(origin.recovered_bytes)
                .is_none_or(|end| end > image.len())
        {
            return Err("Saved carved DAT range is outside the source image".into());
        }
        let mut data = vec![0; origin.recovered_bytes as usize];
        image
            .read_exact_at(origin.image_offset, &mut data)
            .map_err(|e| e.to_string())?;
        Ok(data)
    } else {
        let info = fs::metadata(&metadata.source_path)
            .map_err(|e| format!("Cannot prepare saved DAT: {e}"))?;
        if !info.is_file() || info.len() > MAX_SOURCE_BYTES {
            return Err("Saved DAT is not a supported file up to 512 MiB".into());
        }
        fs::read(&metadata.source_path).map_err(|e| e.to_string())
    }
}
