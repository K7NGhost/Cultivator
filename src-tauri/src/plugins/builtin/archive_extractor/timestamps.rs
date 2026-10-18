use chrono::{Local, NaiveDate, TimeZone};
use std::{
    fs,
    io::Read,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zip::{extra_fields::ExtraField, read::ZipFile};

pub(super) fn zip_modified_time<R: Read>(file: &ZipFile<'_, R>) -> Option<SystemTime> {
    // Prefer absolute timestamps over the timezone-free, two-second DOS timestamp.
    for field in file.extra_data_fields() {
        if let ExtraField::Ntfs(ntfs) = field {
            let ticks = ntfs.mtime();
            let epoch = UNIX_EPOCH.checked_sub(Duration::from_secs(11_644_473_600))?;
            if let Some(time) = epoch.checked_add(Duration::new(
                ticks / 10_000_000,
                ((ticks % 10_000_000) * 100) as u32,
            )) {
                return Some(time);
            }
        }
    }
    for field in file.extra_data_fields() {
        if let ExtraField::ExtendedTimestamp(timestamp) = field {
            if let Some(seconds) = timestamp.mod_time() {
                return UNIX_EPOCH.checked_add(Duration::from_secs(seconds.into()));
            }
        }
    }
    let date = file.last_modified()?;
    let naive =
        NaiveDate::from_ymd_opt(date.year().into(), date.month().into(), date.day().into())?
            .and_hms_opt(
                date.hour().into(),
                date.minute().into(),
                date.second().into(),
            )?;
    // DOS ZIP times are local wall-clock times; use the host timezone when no offset was archived.
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(SystemTime::from)
}

pub(super) fn tar_modified_time<R: Read>(entry: &mut tar::Entry<'_, R>) -> Option<SystemTime> {
    if let Ok(Some(extensions)) = entry.pax_extensions() {
        for extension in extensions.flatten() {
            if extension.key() == Ok("mtime") {
                if let Some(time) = extension.value().ok().and_then(parse_pax_time) {
                    return Some(time);
                }
            }
        }
    }
    UNIX_EPOCH.checked_add(Duration::from_secs(entry.header().mtime().ok()?))
}

fn parse_pax_time(value: &str) -> Option<SystemTime> {
    let negative = value.starts_with('-');
    let value = value.strip_prefix('-').unwrap_or(value);
    let (seconds, fraction) = value.split_once('.').unwrap_or((value, ""));
    if seconds.is_empty()
        || !seconds
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut nanos = 0;
    for i in 0..9 {
        nanos = nanos * 10 + fraction.as_bytes().get(i).map_or(0, |b| (b - b'0') as u32);
    }
    let duration = Duration::new(seconds.parse().ok()?, nanos);
    if negative {
        UNIX_EPOCH.checked_sub(duration)
    } else {
        UNIX_EPOCH.checked_add(duration)
    }
}

pub(super) fn restore_directory_times(
    mut directories: Vec<(PathBuf, SystemTime)>,
    errors: &mut Vec<String>,
) {
    // Creating children changes parent mtimes. Apply the archived times deepest-first after all writes.
    directories.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
    for (path, modified) in directories {
        let result = (|| {
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.custom_flags(0x02000000); // FILE_FLAG_BACKUP_SEMANTICS permits directory handles.
                options.access_mode(0x0100); // FILE_WRITE_ATTRIBUTES is sufficient for SetFileTime.
            }
            options.open(&path)?.set_modified(modified)
        })();
        if let Err(error) = result {
            errors.push(format!(
                "Failed to restore modification time for directory '{}': {error}",
                path.display()
            ));
        }
    }
}

#[cfg(test)]
mod tests;
