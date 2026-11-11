use super::timestamps::{restore_directory_times, zip_modified_time};
use super::{
    archive_entry_matches, sanitized_destination_path, ArchiveEntryMatchers,
    ArchiveExtractionRecord, ProgressThrottle, PROGRESS_INTERVAL,
};
use crate::plugins::cancellation::{self, Check};
use std::time::SystemTime;
use std::{
    collections::{HashMap, HashSet},
    fs,
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Instant,
};
use zip::ZipArchive;

struct EntryTask {
    index: usize,
    name: String,
    destination: PathBuf,
    directory: bool,
    modified: Option<SystemTime>,
}

pub(super) fn extract_zip_archive(
    archive_path: &Path,
    archive_output: &Path,
    errors: &mut Vec<String>,
    matchers: Option<&ArchiveEntryMatchers>,
    cancelled: Check<'_>,
    mut progress: impl FnMut(u64),
) -> Result<ArchiveExtractionRecord, String> {
    cancellation::checkpoint(cancelled)?;
    let mut archive = open_archive(archive_path)?;
    let mut record = ArchiveExtractionRecord {
        archive_path: archive_path.to_string_lossy().into_owned(),
        output_path: archive_output.to_string_lossy().into_owned(),
        entries: archive.len() as u64,
        extracted_files: 0,
        extracted_bytes: 0,
        skipped_entries: 0,
    };
    fs::create_dir_all(archive_output).map_err(|error| {
        format!(
            "Failed to create archive output '{}': {error}",
            archive_output.display()
        )
    })?;
    let mut tasks = Vec::new();
    let mut destinations = HashMap::new();
    let mut parents = HashSet::new();
    let mut collisions = false;
    let mut completed = 0;
    let mut throttle = ProgressThrottle::new();
    for index in 0..archive.len() {
        cancellation::checkpoint(cancelled)?;
        let result: Result<Option<EntryTask>, String> = (|| {
            let file = archive.by_index_raw(index).map_err(|e| e.to_string())?;
            if matchers.is_some_and(|m| !archive_entry_matches(m, file.name())) {
                return Ok(None);
            }
            let enclosed = file
                .enclosed_name()
                .ok_or_else(|| format!("Unsafe ZIP entry '{}'", file.name()))?;
            let destination = sanitized_destination_path(archive_output, &enclosed)
                .ok_or_else(|| format!("ZIP entry '{}' has no usable output path", file.name()))?;
            Ok(Some(EntryTask {
                index,
                name: file.name().to_owned(),
                destination,
                directory: file.is_dir(),
                modified: zip_modified_time(&file),
            }))
        })();
        match result {
            Ok(Some(task)) => {
                // Windows path aliases and sanitized names must never be written concurrently.
                let key = task.destination.to_string_lossy().to_lowercase();
                if destinations.contains_key(&key) || (!task.directory && parents.contains(&key)) {
                    collisions = true;
                }
                for parent in task.destination.ancestors().skip(1) {
                    let parent = parent.to_string_lossy().to_lowercase();
                    if destinations.get(&parent) == Some(&false) {
                        collisions = true;
                    }
                    parents.insert(parent);
                }
                destinations.insert(key, task.directory);
                tasks.push(task);
            }
            Ok(None) => {}
            Err(error) => {
                errors.push(format!(
                    "Failed to read entry {index} from '{}': {error}",
                    archive_path.display()
                ));
                record.skipped_entries += 1;
                completed += 1;
            }
        }
    }

    let mut apply_result = |result: Result<(Option<u64>, Option<String>), String>| match result {
        Ok((bytes, warning)) => {
            if let Some(bytes) = bytes {
                record.extracted_files += 1;
                record.extracted_bytes += bytes;
            }
            if let Some(warning) = warning {
                errors.push(warning);
            }
        }
        Err(error) => {
            record.skipped_entries += 1;
            errors.push(format!("Archive '{}': {error}", archive_path.display()));
        }
    };
    if collisions || tasks.len() < 2 {
        for task in &tasks {
            cancellation::checkpoint(cancelled)?;
            apply_result(extract_entry(&mut archive, task, cancelled));
            completed += 1;
            if throttle.should_emit(Instant::now()) {
                progress(completed);
            }
        }
    } else {
        // Bound both open archive readers and queued results. The callback stays on the caller's thread.
        let workers = crate::jobs::cpu_parallelism().min(tasks.len());
        let next = AtomicUsize::new(0);
        let (sender, receiver) = mpsc::sync_channel(workers * 2);
        thread::scope(|scope| {
            for _ in 0..workers {
                let sender = sender.clone();
                let tasks = &tasks;
                let next = &next;
                scope.spawn(move || {
                    // Each worker owns a file handle and seek position; File::try_clone would share offsets.
                    let mut reader = open_archive(archive_path);
                    loop {
                        if cancelled() {
                            break;
                        }
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(task) = tasks.get(index) else { break };
                        let result = match &mut reader {
                            Ok(reader) => extract_entry(reader, task, cancelled),
                            Err(error) => Err(error.clone()),
                        };
                        if sender.send((index, result)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            let mut results = Vec::with_capacity(tasks.len());
            loop {
                match receiver.recv_timeout(PROGRESS_INTERVAL) {
                    Ok(result) => {
                        results.push(result);
                        completed += 1;
                        // Drain ready results before checking progress, avoiding a callback per entry.
                        for result in receiver.try_iter() {
                            results.push(result);
                            completed += 1;
                        }
                        if throttle.should_emit(Instant::now()) {
                            progress(completed);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => progress(completed),
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            // Preserve diagnostic order even when workers finish out of order.
            results.sort_unstable_by_key(|(index, _)| *index);
            for (_, result) in results {
                apply_result(result);
            }
        });
    }
    cancellation::checkpoint(cancelled)?;
    restore_directory_times(
        tasks
            .iter()
            .filter(|task| task.directory)
            .filter_map(|task| {
                task.modified
                    .map(|modified| (task.destination.clone(), modified))
            })
            .collect(),
        errors,
    );
    progress(completed);
    Ok(record)
}

fn open_archive(path: &Path) -> Result<ZipArchive<fs::File>, String> {
    let input = fs::File::open(path)
        .map_err(|e| format!("Failed to open ZIP '{}': {e}", path.display()))?;
    ZipArchive::new(input).map_err(|e| format!("Failed to read ZIP '{}': {e}", path.display()))
}

fn extract_entry(
    archive: &mut ZipArchive<fs::File>,
    task: &EntryTask,
    cancelled: Check<'_>,
) -> Result<(Option<u64>, Option<String>), String> {
    let mut output_created = false;
    let extract = || -> Result<(Option<u64>, Option<String>), String> {
        let mut file = archive.by_index(task.index).map_err(|e| e.to_string())?;
        if task.directory {
            fs::create_dir_all(&task.destination).map_err(|e| e.to_string())?;
            return Ok((None, None));
        }
        let parent = task
            .destination
            .parent()
            .ok_or("Output path has no parent")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut output = fs::File::create(&task.destination).map_err(|e| e.to_string())?;
        output_created = true;
        let bytes =
            cancellation::copy(&mut file, &mut output, cancelled).map_err(|e| e.to_string())?;
        let warning = task
            .modified
            .and_then(|modified| output.set_modified(modified).err())
            .map(|error| {
                format!(
                    "Failed to restore modification time for '{}': {error}",
                    task.destination.display()
                )
            });
        Ok((Some(bytes), warning))
    };
    // Preserve a decoder panic's reason before thread::scope can replace it
    // with the unhelpful "a scoped thread panicked" message.
    let result = catch_unwind(AssertUnwindSafe(extract)).unwrap_or_else(|panic| {
        let reason = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap_or("unknown decoder panic");
        Err(format!("ZIP decoder panicked: {reason}"))
    });
    result.map_err(|mut error| {
        // Failed reads, CRC checks, cancellation and decoder panics must never
        // leave a truncated member looking like a successfully extracted file.
        if output_created {
            if let Err(cleanup) = fs::remove_file(&task.destination) {
                error.push_str(&format!("; failed to remove incomplete output: {cleanup}"));
            }
        }
        format!(
            "Failed to extract ZIP entry {} ('{}') to '{}': {error}",
            task.index,
            task.name,
            task.destination.display()
        )
    })
}

#[cfg(test)]
mod tests;
