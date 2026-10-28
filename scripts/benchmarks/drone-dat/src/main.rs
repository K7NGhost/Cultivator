//! Runs the production DAT decoder without Tauri. Outputs go only to the supplied directory.
#![allow(dead_code)]
use cultivator_drone_dat as dat;
use inodex::image::{DiskImage, Image};
use serde_json::{json, Value};
use std::{fs, path::Path, time::Instant};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err(
            "Usage: cultivator-drone-benchmark parse|preview|export|carve|full|csv SOURCE OUTPUT_DIRECTORY".into(),
        );
    }
    let source = Path::new(&args[2]);
    let output = Path::new(&args[3]);
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let mut results = Vec::<Value>::new();
    let mut scan_seconds = 0.0;
    if matches!(args[1].as_str(), "parse" | "preview" | "export") {
        let dataset = dat::convert_file(source, output)?;
        let decode_seconds = started.elapsed().as_secs_f64();
        let cache_bytes = fs::metadata(dataset.csv_path().with_file_name("telemetry.bin"))
            .map_err(|e| e.to_string())?
            .len();
        let reopen = Instant::now();
        let reopened = dat::restore_file(output, &dataset.metadata.id)?;
        let reopen_seconds = reopen.elapsed().as_secs_f64();
        let page = Instant::now();
        let _ = reopened.page(
            reopened.metadata.row_count.saturating_sub(100) / 100,
            100,
            "",
            "",
        )?;
        let page_seconds = page.elapsed().as_secs_f64();
        let export = Instant::now();
        let csv_hash = if args[1] == "export" {
            Some(reopened.export_csv(&dataset.csv_path())?)
        } else {
            None
        };
        results.push(json!({"metadata": dataset.metadata, "decodeSeconds":decode_seconds,
            "cacheBytes":cache_bytes,"reopenSeconds":reopen_seconds,"pageSeconds":page_seconds,
            "routeSamples":reopened.route.len(),"chartSamples":reopened.samples.len(),
            "exportSeconds":if csv_hash.is_some() {Some(export.elapsed().as_secs_f64())} else {None},
            "exportCsvSha256":csv_hash, "elapsedSeconds": started.elapsed().as_secs_f64()}));
    } else {
        if args[1] != "carve" && args[1] != "full" && args[1] != "csv" {
            return Err("Choose parse, preview, carve, full, or csv".into());
        }
        let mut image = Image::open(source).map_err(|e| e.to_string())?;
        let candidates = dat::carving::scan(&mut image, &|| false, |_, _, _| {})?;
        scan_seconds = started.elapsed().as_secs_f64();
        eprintln!(
            "Scan: {} candidates in {scan_seconds:.3}s",
            candidates.len()
        );
        for (index, candidate) in candidates.iter().enumerate() {
            let recovery_start = Instant::now();
            let next = candidates.get(index + 1).map_or(image.len(), |c| c.offset);
            let recovered = dat::carving::recover(
                &mut image,
                candidate,
                next,
                dat::MAX_SOURCE_BYTES,
                &|| false,
                |_| {},
            )?;
            let recover_seconds = recovery_start.elapsed().as_secs_f64();
            let parse_start = Instant::now();
            let metadata = if args[1] == "full" || args[1] == "csv" {
                let path = output.join(format!("0x{:x}.DAT", candidate.offset));
                let converted = if args[1] == "full" {
                    dat::convert_carved_bytes_checked(
                        &path,
                        &recovered.bytes,
                        output,
                        &|| false,
                        dat::CarvingProvenance {
                            image_path: source.to_string_lossy().into_owned(),
                            image_offset: candidate.offset,
                            recovered_bytes: recovered.bytes.len() as u64,
                            stopped_at_size_limit: recovered.stopped_at_size_limit,
                            stored_as_range: true,
                        },
                    )
                } else {
                    fs::write(&path, &recovered.bytes).map_err(|e| e.to_string())?;
                    dat::convert_bytes_checked(&path, &recovered.bytes, output, &|| false)
                };
                match converted {
                    Ok(dataset) => {
                        if args[1] == "csv" {
                            dataset.export_csv(&dataset.csv_path())?;
                        }
                        json!(dataset.metadata)
                    }
                    Err(error) => json!({"error": error}),
                }
            } else {
                Value::Null
            };
            eprintln!(
                "{}/{} 0x{:x}: {} bytes, recover {recover_seconds:.3}s, decode {:.3}s",
                index + 1,
                candidates.len(),
                candidate.offset,
                recovered.bytes.len(),
                parse_start.elapsed().as_secs_f64()
            );
            results.push(json!({"offset":candidate.offset, "format":candidate.format,
                "bytes":recovered.bytes.len(), "fingerprint":recovered.fingerprint,
                "limited":recovered.stopped_at_size_limit, "recoverSeconds":recover_seconds,
                "parseSeconds":parse_start.elapsed().as_secs_f64(), "metadata":metadata}));
        }
    }
    let report = json!({"mode":args[1], "source":source, "elapsedSeconds":started.elapsed().as_secs_f64(), "scanSeconds":scan_seconds, "results":results});
    fs::write(
        output.join("benchmark.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!("{}", report);
    Ok(())
}
