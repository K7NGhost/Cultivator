use super::{
    add_missing_manifest_metadata, builtin, create_id, discover_python_plugin_directories,
    ensure_not_inside_plugin_directory, is_safe_plugin_id, normalize_toml_for_write,
    validate_manifest, LoadedPythonPlugin, PythonPluginManifest,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    sync::{Mutex, MutexGuard, OnceLock},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const BUNDLE_MARKER_NAME: &str = "cultivator-plugin-bundle.json";
const BUNDLE_FORMAT: &str = "cultivator-python-plugins";
const BUNDLE_FORMAT_VERSION: u32 = 1;
const ROOT_API_STUB_NAME: &str = "cultivator_api.pyi";
const ROOT_API_GUIDE_NAME: &str = "cultivator_python_api_guide.html";
const MAX_PLUGIN_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_CENTRAL_DIRECTORY_BYTES: u64 = 128 * 1024 * 1024;
const ZIP_EOCD_SIGNATURE: u32 = 0x0605_4b50;
const ZIP_CENTRAL_FILE_SIGNATURE: u32 = 0x0201_4b50;
const ZIP_EOCD_SIZE: usize = 22;
const ZIP_CENTRAL_FILE_HEADER_SIZE: usize = 46;

static BUNDLE_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Copy)]
struct BundleLimits {
    max_archive_bytes: u64,
    max_entries: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
    max_marker_bytes: u64,
    max_path_bytes: usize,
}

impl Default for BundleLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 1024 * 1024 * 1024,
            max_entries: 20_000,
            max_file_bytes: 512 * 1024 * 1024,
            max_total_bytes: 2 * 1024 * 1024 * 1024,
            max_marker_bytes: 1024 * 1024,
            max_path_bytes: 4_096,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPythonPluginsRequest {
    pub archive_path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPythonPluginsRequest {
    pub archive_path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginBundleExportResult {
    pub archive_path: String,
    pub plugin_count: usize,
    pub file_count: usize,
    pub byte_count: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedPythonPlugin {
    pub id: String,
    pub directory: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginBundleImportResult {
    pub archive_path: String,
    pub plugin_count: usize,
    pub file_count: usize,
    pub byte_count: u64,
    pub plugins: Vec<ImportedPythonPlugin>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BundleMarker {
    format: String,
    format_version: u32,
    plugins: Vec<BundlePluginDescriptor>,
    support_files: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BundlePluginDescriptor {
    id: String,
    path: String,
}

#[derive(Clone)]
struct BundleFile {
    source_path: PathBuf,
    relative_path: PathBuf,
    archive_path: String,
    size: u64,
}

#[derive(Clone)]
struct ArchiveEntry {
    index: usize,
    relative_path: PathBuf,
    archive_path: String,
    size: u64,
    is_directory: bool,
}

struct CleanupPath {
    path: PathBuf,
    directory: bool,
    armed: bool,
}

impl CleanupPath {
    fn file(path: PathBuf) -> Self {
        Self {
            path,
            directory: false,
            armed: true,
        }
    }

    fn directory(path: PathBuf) -> Self {
        Self {
            path,
            directory: true,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CleanupPath {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }

        if self.directory {
            let _ = fs::remove_dir_all(&self.path);
        } else {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) fn export_python_plugins(
    plugin_root: &Path,
    plugins: &[LoadedPythonPlugin],
    request: ExportPythonPluginsRequest,
) -> Result<PluginBundleExportResult, String> {
    export_python_plugins_with_limits(plugin_root, plugins, request, BundleLimits::default())
}

pub(super) fn import_python_plugins(
    plugin_root: &Path,
    installed_plugins: &[LoadedPythonPlugin],
    request: ImportPythonPluginsRequest,
) -> Result<PluginBundleImportResult, String> {
    import_python_plugins_with_limits(
        plugin_root,
        installed_plugins,
        request,
        BundleLimits::default(),
    )
}

pub(super) fn lock_bundle_operations() -> Result<MutexGuard<'static, ()>, String> {
    BUNDLE_OPERATION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| "Python plugin bundle operation lock is unavailable.".to_string())
}

fn export_python_plugins_with_limits(
    plugin_root: &Path,
    plugins: &[LoadedPythonPlugin],
    request: ExportPythonPluginsRequest,
    limits: BundleLimits,
) -> Result<PluginBundleExportResult, String> {
    if plugins.is_empty() {
        return Err("There are no custom Python plugins to export.".to_string());
    }

    let archive_path = validated_archive_path(&request.archive_path, false)?;
    let archive_parent = archive_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "The ZIP archive must have a parent directory.".to_string())?;
    if !archive_parent.is_dir() {
        return Err(format!(
            "The ZIP archive parent directory does not exist: {}",
            archive_parent.display()
        ));
    }

    let canonical_archive_parent = archive_parent.canonicalize().map_err(|error| {
        format!(
            "Failed to inspect ZIP archive parent '{}': {error}",
            archive_parent.display()
        )
    })?;
    let archive_name = archive_path
        .file_name()
        .ok_or_else(|| "The ZIP archive requires a file name.".to_string())?;
    let canonical_archive_path = canonical_archive_parent.join(archive_name);
    let canonical_plugin_root = plugin_root.canonicalize().map_err(|error| {
        format!(
            "Failed to inspect Python plugin directory '{}': {error}",
            plugin_root.display()
        )
    })?;
    if canonical_archive_path.starts_with(&canonical_plugin_root) {
        return Err(
            "The export archive cannot be saved inside the Python plugin directory.".to_string(),
        );
    }
    if let Some(metadata) = symlink_metadata_if_present(&archive_path)? {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "The export destination is not a regular file: {}",
                archive_path.display()
            ));
        }
    }

    for plugin in plugins {
        let plugin_directory = plugin.directory.canonicalize().map_err(|error| {
            format!(
                "Failed to inspect Python plugin '{}': {error}",
                plugin.manifest.id
            )
        })?;
        if canonical_archive_path.starts_with(&plugin_directory) {
            return Err(format!(
                "The export archive cannot be saved inside Python plugin '{}'.",
                plugin.manifest.id
            ));
        }
    }

    let (marker, files) = collect_export_files(plugin_root, plugins, limits)?;
    let marker_bytes = serialize_bundle_marker(&marker, limits)?;
    let payload_bytes = files
        .iter()
        .try_fold(marker_bytes.len() as u64, |total, file| {
            total
                .checked_add(file.size)
                .ok_or_else(|| "The total size of plugin bundle files is too large.".to_string())
        })?;
    if payload_bytes > limits.max_total_bytes {
        return Err(format!(
            "Plugin bundle contents exceed the {} byte limit.",
            limits.max_total_bytes
        ));
    }
    let temporary_name = format!(
        ".{}.{}.tmp",
        archive_name.to_string_lossy(),
        create_id("plugin-export")
    );
    let temporary_path = canonical_archive_parent.join(temporary_name);
    let temporary_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .read(true)
        .open(&temporary_path)
        .map_err(|error| {
            format!(
                "Failed to create temporary plugin archive '{}': {error}",
                temporary_path.display()
            )
        })?;
    let mut cleanup = CleanupPath::file(temporary_path.clone());

    write_bundle_archive(temporary_file, &marker_bytes, &files)?;

    let byte_count = fs::metadata(&temporary_path)
        .map_err(|error| format!("Failed to inspect temporary plugin archive: {error}"))?
        .len();
    if byte_count > limits.max_archive_bytes {
        return Err(format!(
            "The plugin archive is too large ({byte_count} bytes; maximum is {} bytes).",
            limits.max_archive_bytes
        ));
    }

    replace_export_archive(&temporary_path, &archive_path)?;
    cleanup.disarm();

    Ok(PluginBundleExportResult {
        archive_path: archive_path.to_string_lossy().to_string(),
        plugin_count: marker.plugins.len(),
        file_count: files.len(),
        byte_count,
    })
}

fn import_python_plugins_with_limits(
    plugin_root: &Path,
    installed_plugins: &[LoadedPythonPlugin],
    request: ImportPythonPluginsRequest,
    limits: BundleLimits,
) -> Result<PluginBundleImportResult, String> {
    let archive_path = validated_archive_path(&request.archive_path, true)?;
    let canonical_plugin_root = plugin_root.canonicalize().map_err(|error| {
        format!(
            "Failed to inspect Python plugin directory '{}': {error}",
            plugin_root.display()
        )
    })?;
    let canonical_archive_path = archive_path.canonicalize().map_err(|error| {
        format!(
            "Failed to inspect plugin archive '{}': {error}",
            archive_path.display()
        )
    })?;
    if canonical_archive_path.starts_with(&canonical_plugin_root) {
        return Err(
            "Plugin bundles cannot be imported from inside the Python plugin directory."
                .to_string(),
        );
    }
    let archive_metadata = fs::metadata(&archive_path).map_err(|error| {
        format!(
            "Failed to inspect plugin archive '{}': {error}",
            archive_path.display()
        )
    })?;
    if archive_metadata.len() > limits.max_archive_bytes {
        return Err(format!(
            "The plugin archive is too large ({} bytes; maximum is {} bytes).",
            archive_metadata.len(),
            limits.max_archive_bytes
        ));
    }

    let mut input = File::open(&archive_path).map_err(|error| {
        format!(
            "Failed to open plugin archive '{}': {error}",
            archive_path.display()
        )
    })?;
    preflight_zip_archive(&mut input, archive_metadata.len(), limits)?;
    let mut archive = ZipArchive::new(input).map_err(|error| {
        format!(
            "Failed to read plugin archive '{}': {error}",
            archive_path.display()
        )
    })?;
    let entries = inspect_archive(&mut archive, limits)?;
    let marker = read_bundle_marker(&mut archive, &entries, limits)?;
    validate_bundle_marker(&marker, &entries)?;

    let staging_parent = plugin_root.parent().ok_or_else(|| {
        "The Python plugin directory must have a parent directory for staging.".to_string()
    })?;
    let staging_root = staging_parent.join(format!(".{}", create_id("python-plugin-import")));
    fs::create_dir(&staging_root).map_err(|error| {
        format!(
            "Failed to create plugin import staging directory '{}': {error}",
            staging_root.display()
        )
    })?;
    let mut staging_cleanup = CleanupPath::directory(staging_root.clone());

    extract_archive_to_staging(&mut archive, &entries, &staging_root, limits)?;
    require_exact_discovered_plugins(&staging_root, &marker.plugins, limits.max_path_bytes)?;
    let staged_plugins = validate_staged_plugins(&staging_root, &marker)?;
    preflight_import(
        plugin_root,
        &staging_root,
        installed_plugins,
        &marker,
        &staged_plugins,
    )?;
    let imported = install_staged_bundle(plugin_root, &staging_root, &marker, &staged_plugins)?;

    if fs::remove_dir_all(&staging_root).is_ok() {
        staging_cleanup.disarm();
    }

    Ok(PluginBundleImportResult {
        archive_path: archive_path.to_string_lossy().to_string(),
        plugin_count: imported.len(),
        file_count: entries
            .iter()
            .filter(|entry| !entry.is_directory && entry.archive_path != BUNDLE_MARKER_NAME)
            .count(),
        byte_count: archive_metadata.len(),
        plugins: imported,
    })
}

fn preflight_zip_archive(
    input: &mut File,
    archive_size: u64,
    limits: BundleLimits,
) -> Result<(), String> {
    if archive_size < ZIP_EOCD_SIZE as u64 {
        return Err("Plugin archive is too small to be a valid ZIP file.".to_string());
    }

    let tail_size = archive_size.min((ZIP_EOCD_SIZE + u16::MAX as usize) as u64);
    input
        .seek(SeekFrom::End(-(tail_size as i64)))
        .map_err(|error| format!("Failed to seek plugin archive: {error}"))?;
    let mut tail = vec![0u8; tail_size as usize];
    input
        .read_exact(&mut tail)
        .map_err(|error| format!("Failed to read plugin archive footer: {error}"))?;

    let eocd_offset_in_tail = (0..=tail.len() - ZIP_EOCD_SIZE)
        .rev()
        .find(|offset| {
            read_u32(&tail[*offset..*offset + 4]) == ZIP_EOCD_SIGNATURE
                && *offset + ZIP_EOCD_SIZE + read_u16(&tail[*offset + 20..*offset + 22]) as usize
                    == tail.len()
        })
        .ok_or_else(|| "Plugin archive has no valid ZIP end record.".to_string())?;
    let eocd = &tail[eocd_offset_in_tail..eocd_offset_in_tail + ZIP_EOCD_SIZE];
    let disk_number = read_u16(&eocd[4..6]);
    let central_disk = read_u16(&eocd[6..8]);
    let entries_on_disk = read_u16(&eocd[8..10]);
    let total_entries = read_u16(&eocd[10..12]);
    let central_size = u64::from(read_u32(&eocd[12..16]));
    let central_offset = u64::from(read_u32(&eocd[16..20]));

    if disk_number != 0 || central_disk != 0 || entries_on_disk != total_entries {
        return Err("Multi-disk ZIP archives are not supported for plugin bundles.".to_string());
    }
    if total_entries == u16::MAX
        || central_size == u64::from(u32::MAX)
        || central_offset == u64::from(u32::MAX)
    {
        return Err("ZIP64 plugin bundles are not supported.".to_string());
    }
    if total_entries == 0 {
        return Err("Plugin archive is empty.".to_string());
    }
    if total_entries as usize > limits.max_entries {
        return Err(format!(
            "Plugin archive contains too many entries ({}; maximum is {}).",
            total_entries, limits.max_entries
        ));
    }
    if central_size > MAX_CENTRAL_DIRECTORY_BYTES {
        return Err(format!(
            "Plugin archive central directory exceeds the {} byte limit.",
            MAX_CENTRAL_DIRECTORY_BYTES
        ));
    }

    let eocd_absolute_offset = archive_size - tail_size + eocd_offset_in_tail as u64;
    let central_end = central_offset
        .checked_add(central_size)
        .ok_or_else(|| "Plugin archive central directory size overflowed.".to_string())?;
    if central_end != eocd_absolute_offset {
        return Err("Plugin archive has an invalid central directory boundary.".to_string());
    }

    input
        .seek(SeekFrom::Start(central_offset))
        .map_err(|error| format!("Failed to seek plugin archive central directory: {error}"))?;
    let mut cursor = central_offset;
    let mut actual_entries = 0usize;
    while cursor < central_end {
        if actual_entries >= limits.max_entries
            || central_end - cursor < ZIP_CENTRAL_FILE_HEADER_SIZE as u64
        {
            return Err(
                "Plugin archive central directory has too many or truncated entries.".to_string(),
            );
        }

        let mut header = [0u8; ZIP_CENTRAL_FILE_HEADER_SIZE];
        input
            .read_exact(&mut header)
            .map_err(|error| format!("Failed to read ZIP central directory entry: {error}"))?;
        if read_u32(&header[0..4]) != ZIP_CENTRAL_FILE_SIGNATURE {
            return Err("Plugin archive central directory contains an invalid entry.".to_string());
        }
        let name_length = u64::from(read_u16(&header[28..30]));
        let extra_length = u64::from(read_u16(&header[30..32]));
        let comment_length = u64::from(read_u16(&header[32..34]));
        if name_length == 0 || name_length > limits.max_path_bytes as u64 + 1 {
            return Err(
                "Plugin archive central directory contains an invalid path length.".to_string(),
            );
        }
        let entry_size = (ZIP_CENTRAL_FILE_HEADER_SIZE as u64)
            .checked_add(name_length)
            .and_then(|size| size.checked_add(extra_length))
            .and_then(|size| size.checked_add(comment_length))
            .ok_or_else(|| "Plugin archive central entry size overflowed.".to_string())?;
        cursor = cursor
            .checked_add(entry_size)
            .ok_or_else(|| "Plugin archive central directory position overflowed.".to_string())?;
        if cursor > central_end {
            return Err("Plugin archive central directory entry exceeds its boundary.".to_string());
        }
        input
            .seek(SeekFrom::Start(cursor))
            .map_err(|error| format!("Failed to seek ZIP central directory entry: {error}"))?;
        actual_entries += 1;
    }
    if actual_entries != total_entries as usize {
        return Err(format!(
            "Plugin archive entry count mismatch (footer: {total_entries}, actual: {actual_entries})."
        ));
    }

    input
        .seek(SeekFrom::Start(0))
        .map_err(|error| format!("Failed to rewind plugin archive: {error}"))?;
    Ok(())
}

fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn validated_archive_path(value: &str, must_exist: bool) -> Result<PathBuf, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("ZIP archive path is required.".to_string());
    }

    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("ZIP archive path must be absolute.".to_string());
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("zip"))
    {
        return Err("Plugin bundles must use the .zip extension.".to_string());
    }
    if must_exist {
        let metadata = symlink_metadata_if_present(&path)?
            .ok_or_else(|| format!("Plugin archive was not found: {}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "Plugin archive is not a regular file: {}",
                path.display()
            ));
        }
    }

    Ok(path)
}

fn collect_export_files(
    plugin_root: &Path,
    plugins: &[LoadedPythonPlugin],
    limits: BundleLimits,
) -> Result<(BundleMarker, Vec<BundleFile>), String> {
    let canonical_root = plugin_root.canonicalize().map_err(|error| {
        format!(
            "Failed to inspect Python plugin directory '{}': {error}",
            plugin_root.display()
        )
    })?;
    let mut descriptors = Vec::with_capacity(plugins.len());
    let mut plugin_paths = Vec::with_capacity(plugins.len());
    let mut required_files = Vec::with_capacity(plugins.len() * 2);
    let mut plugin_ids = HashSet::new();

    for plugin in plugins {
        validate_manifest(&plugin.manifest, &plugin.directory)?;
        if conflicts_with_builtin_id(&plugin.manifest.id) {
            return Err(format!(
                "Built-in plugin '{}' cannot be exported.",
                plugin.manifest.id
            ));
        }
        if !plugin_ids.insert(plugin.manifest.id.to_ascii_lowercase()) {
            return Err(format!(
                "Python plugin id '{}' is installed more than once.",
                plugin.manifest.id
            ));
        }

        let canonical_directory = plugin.directory.canonicalize().map_err(|error| {
            format!(
                "Failed to inspect Python plugin '{}': {error}",
                plugin.manifest.id
            )
        })?;
        let relative_path = canonical_directory
            .strip_prefix(&canonical_root)
            .map_err(|_| {
                format!(
                    "Python plugin '{}' is outside the Python plugin directory.",
                    plugin.manifest.id
                )
            })?
            .to_path_buf();
        if relative_path.as_os_str().is_empty() {
            return Err(format!(
                "Python plugin '{}' cannot use the plugin root as its directory.",
                plugin.manifest.id
            ));
        }
        let archive_path = portable_relative_path(&relative_path, limits.max_path_bytes)?;
        descriptors.push(BundlePluginDescriptor {
            id: plugin.manifest.id.clone(),
            path: archive_path.clone(),
        });
        required_files.push(format!("{archive_path}/plugin.toml"));
        let entry_relative_path = portable_relative_path(
            Path::new(plugin.manifest.entry.trim()),
            limits.max_path_bytes,
        )?;
        required_files.push(format!("{archive_path}/{entry_relative_path}"));
        plugin_paths.push(relative_path);
    }

    descriptors.sort_by(|left, right| left.path.cmp(&right.path));
    plugin_paths.sort();
    reject_overlapping_plugin_paths(&descriptors)?;
    require_exact_discovered_plugins(&canonical_root, &descriptors, limits.max_path_bytes)?;

    let mut files = collect_regular_files(&canonical_root, limits)?;
    let mut total_bytes = 0u64;
    let mut support_files = Vec::new();
    let mut exported_path_keys = HashSet::new();

    for file in &files {
        if !exported_path_keys.insert(file.archive_path.to_lowercase()) {
            return Err(format!(
                "Plugin files collide on a case-insensitive filesystem path: {}",
                file.archive_path
            ));
        }
        total_bytes = total_bytes
            .checked_add(file.size)
            .ok_or_else(|| "The total size of plugin bundle files is too large.".to_string())?;
        if file.size > limits.max_file_bytes {
            return Err(format!(
                "Plugin bundle file '{}' is too large ({} bytes; maximum is {} bytes).",
                file.archive_path, file.size, limits.max_file_bytes
            ));
        }
        if total_bytes > limits.max_total_bytes {
            return Err(format!(
                "Plugin bundle contents exceed the {} byte limit.",
                limits.max_total_bytes
            ));
        }

        if !plugin_paths
            .iter()
            .any(|plugin_path| file.relative_path.starts_with(plugin_path))
        {
            support_files.push(file.archive_path.clone());
        }
    }

    if files.len() + 1 > limits.max_entries {
        return Err(format!(
            "Plugin bundle contains too many files ({}; maximum is {}).",
            files.len(),
            limits.max_entries.saturating_sub(1)
        ));
    }

    files.sort_by(|left, right| left.archive_path.cmp(&right.archive_path));
    let exported_paths = files
        .iter()
        .map(|file| file.archive_path.to_lowercase())
        .collect::<HashSet<_>>();
    for required_file in required_files {
        if !exported_paths.contains(&required_file.to_lowercase()) {
            return Err(format!(
                "Required plugin file is excluded from the bundle: {required_file}"
            ));
        }
    }
    support_files.sort();

    Ok((
        BundleMarker {
            format: BUNDLE_FORMAT.to_string(),
            format_version: BUNDLE_FORMAT_VERSION,
            plugins: descriptors,
            support_files,
        },
        files,
    ))
}

fn collect_regular_files(
    plugin_root: &Path,
    limits: BundleLimits,
) -> Result<Vec<BundleFile>, String> {
    let mut files = Vec::new();
    let mut pending_directories = vec![plugin_root.to_path_buf()];

    while let Some(directory) = pending_directories.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| format!("Failed to read '{}': {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                format!(
                    "Failed to inspect Python plugin directory '{}': {error}",
                    directory.display()
                )
            })?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let relative_path = path
                .strip_prefix(plugin_root)
                .map_err(|_| "Plugin bundle path escaped the plugin root.".to_string())?
                .to_path_buf();
            let file_type = entry.file_type().map_err(|error| {
                format!(
                    "Failed to inspect plugin file '{}': {error}",
                    path.display()
                )
            })?;
            if path_uses_reserved_root(&relative_path) {
                if is_seeded_root_file(&relative_path) && file_type.is_file() {
                    continue;
                }
                return Err(format!(
                    "The path '{}' is reserved for generated Cultivator plugin files.",
                    relative_path.display()
                ));
            }
            if is_export_excluded(&relative_path) {
                continue;
            }

            if file_type.is_symlink() {
                return Err(format!(
                    "Plugin bundle export does not allow symbolic links: {}",
                    path.display()
                ));
            }
            if file_type.is_dir() {
                pending_directories.push(path);
                continue;
            }
            if !file_type.is_file() {
                return Err(format!(
                    "Plugin bundle export only supports regular files: {}",
                    path.display()
                ));
            }

            let metadata = entry.metadata().map_err(|error| {
                format!(
                    "Failed to inspect plugin file '{}': {error}",
                    path.display()
                )
            })?;
            let archive_path = portable_relative_path(&relative_path, limits.max_path_bytes)?;
            files.push(BundleFile {
                source_path: path,
                relative_path,
                archive_path,
                size: metadata.len(),
            });
        }
    }

    Ok(files)
}

fn is_export_excluded(relative_path: &Path) -> bool {
    let components = relative_path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    if components
        .iter()
        .any(|component| component.eq_ignore_ascii_case("__pycache__"))
    {
        return true;
    }

    let Some(file_name) = components.last() else {
        return false;
    };
    if file_name.eq_ignore_ascii_case(".DS_Store") {
        return true;
    }
    if components.len() == 1
        && (file_name.eq_ignore_ascii_case(ROOT_API_STUB_NAME)
            || file_name.eq_ignore_ascii_case(ROOT_API_GUIDE_NAME))
    {
        return true;
    }

    relative_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("pyc") || extension.eq_ignore_ascii_case("pyo")
        })
}

fn path_uses_reserved_root(relative_path: &Path) -> bool {
    relative_path
        .components()
        .next()
        .and_then(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .is_some_and(|component| {
            component.eq_ignore_ascii_case(BUNDLE_MARKER_NAME)
                || component.eq_ignore_ascii_case(ROOT_API_STUB_NAME)
                || component.eq_ignore_ascii_case(ROOT_API_GUIDE_NAME)
        })
}

fn is_seeded_root_file(relative_path: &Path) -> bool {
    let mut components = relative_path.components();
    let Some(Component::Normal(component)) = components.next() else {
        return false;
    };
    components.next().is_none()
        && component.to_str().is_some_and(|component| {
            component.eq_ignore_ascii_case(ROOT_API_STUB_NAME)
                || component.eq_ignore_ascii_case(ROOT_API_GUIDE_NAME)
        })
}

fn require_exact_discovered_plugins(
    plugin_root: &Path,
    descriptors: &[BundlePluginDescriptor],
    max_path_bytes: usize,
) -> Result<(), String> {
    if plugin_root.join("plugin.toml").exists() {
        return Err(
            "The Python plugin root contains plugin.toml, which is not a valid plugin location."
                .to_string(),
        );
    }

    let discovered = discover_python_plugin_directories(plugin_root)?
        .into_iter()
        .map(|directory| {
            let relative = directory.strip_prefix(plugin_root).map_err(|_| {
                "Discovered Python plugin escaped the Python plugin directory.".to_string()
            })?;
            portable_relative_path(relative, max_path_bytes)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let discovered_paths = discovered
        .iter()
        .map(|path| path.to_lowercase())
        .collect::<HashSet<_>>();
    let declared_paths = descriptors
        .iter()
        .map(|descriptor| descriptor.path.to_lowercase())
        .collect::<HashSet<_>>();
    if discovered_paths != declared_paths {
        let mut missing = discovered_paths
            .difference(&declared_paths)
            .cloned()
            .collect::<Vec<_>>();
        missing.sort();
        let mut stale = declared_paths
            .difference(&discovered_paths)
            .cloned()
            .collect::<Vec<_>>();
        stale.sort();
        return Err(format!(
            "Installed plugin discovery changed while preparing the bundle (undeclared: {}; missing: {}).",
            missing.join(", "),
            stale.join(", ")
        ));
    }
    Ok(())
}

fn conflicts_with_builtin_id(plugin_id: &str) -> bool {
    builtin::plugins()
        .iter()
        .any(|plugin| plugin.manifest().id.eq_ignore_ascii_case(plugin_id))
}

fn serialize_bundle_marker(marker: &BundleMarker, limits: BundleLimits) -> Result<Vec<u8>, String> {
    let marker_bytes = serde_json::to_vec_pretty(marker)
        .map_err(|error| format!("Failed to serialize plugin bundle marker: {error}"))?;
    if marker_bytes.len() as u64 > limits.max_marker_bytes {
        return Err(format!(
            "Plugin bundle marker is too large ({} bytes; maximum is {} bytes).",
            marker_bytes.len(),
            limits.max_marker_bytes
        ));
    }
    Ok(marker_bytes)
}

fn write_bundle_archive(
    output: File,
    marker_bytes: &[u8],
    files: &[BundleFile],
) -> Result<(), String> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut archive = ZipWriter::new(output);
    archive
        .start_file(BUNDLE_MARKER_NAME, options)
        .map_err(|error| format!("Failed to start plugin bundle marker: {error}"))?;
    archive
        .write_all(marker_bytes)
        .map_err(|error| format!("Failed to write plugin bundle marker: {error}"))?;

    for file in files {
        archive
            .start_file(&file.archive_path, options)
            .map_err(|error| {
                format!(
                    "Failed to add '{}' to plugin bundle: {error}",
                    file.archive_path
                )
            })?;
        let input = File::open(&file.source_path).map_err(|error| {
            format!(
                "Failed to open plugin file '{}': {error}",
                file.source_path.display()
            )
        })?;
        let copied = io::copy(
            &mut BufReader::new(input).take(file.size.saturating_add(1)),
            &mut archive,
        )
        .map_err(|error| {
            format!(
                "Failed to add '{}' to plugin bundle: {error}",
                file.archive_path
            )
        })?;
        if copied != file.size {
            return Err(format!(
                "Plugin file '{}' changed while it was being exported.",
                file.source_path.display()
            ));
        }
    }

    let output = archive
        .finish()
        .map_err(|error| format!("Failed to finish plugin bundle: {error}"))?;
    output
        .sync_all()
        .map_err(|error| format!("Failed to flush plugin bundle to disk: {error}"))
}

fn replace_export_archive(temporary_path: &Path, archive_path: &Path) -> Result<(), String> {
    let Some(metadata) = symlink_metadata_if_present(archive_path)? else {
        return fs::rename(temporary_path, archive_path).map_err(|error| {
            format!(
                "Failed to finish plugin archive '{}': {error}",
                archive_path.display()
            )
        });
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "The export destination is not a regular file: {}",
            archive_path.display()
        ));
    }

    let parent = archive_path
        .parent()
        .ok_or_else(|| "The ZIP archive must have a parent directory.".to_string())?;
    let file_name = archive_path
        .file_name()
        .ok_or_else(|| "The ZIP archive requires a file name.".to_string())?;
    let backup_path = parent.join(format!(
        ".{}.{}.backup",
        file_name.to_string_lossy(),
        create_id("plugin-export")
    ));
    fs::rename(archive_path, &backup_path).map_err(|error| {
        format!(
            "Failed to prepare existing plugin archive '{}' for replacement: {error}",
            archive_path.display()
        )
    })?;
    let mut backup_cleanup = CleanupPath::file(backup_path.clone());

    if let Err(error) = fs::rename(temporary_path, archive_path) {
        let rollback = fs::rename(&backup_path, archive_path);
        if rollback.is_ok() {
            backup_cleanup.disarm();
            return Err(format!(
                "Failed to replace plugin archive '{}': {error}",
                archive_path.display()
            ));
        }
        backup_cleanup.disarm();
        return Err(format!(
            "Failed to replace plugin archive '{}': {error}. The previous archive remains at '{}'.",
            archive_path.display(),
            backup_path.display()
        ));
    }

    if fs::remove_file(&backup_path).is_ok() {
        backup_cleanup.disarm();
    }
    Ok(())
}

fn inspect_archive(
    archive: &mut ZipArchive<File>,
    limits: BundleLimits,
) -> Result<Vec<ArchiveEntry>, String> {
    if archive.is_empty() {
        return Err("Plugin archive is empty.".to_string());
    }
    if archive.len() > limits.max_entries {
        return Err(format!(
            "Plugin archive contains too many entries ({}; maximum is {}).",
            archive.len(),
            limits.max_entries
        ));
    }

    let mut entries = Vec::with_capacity(archive.len());
    let mut normalized_paths = HashSet::new();
    let mut total_bytes = 0u64;

    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|error| format!("Failed to inspect ZIP entry {index}: {error}"))?;
        if file.encrypted() {
            return Err(format!(
                "Encrypted ZIP entries are not supported: {}",
                file.name()
            ));
        }
        if file.is_symlink() {
            return Err(format!(
                "Symbolic links are not allowed in plugin bundles: {}",
                file.name()
            ));
        }

        if let Some(mode) = file.unix_mode() {
            let file_kind = mode & 0o170000;
            if file_kind != 0 && file_kind != 0o100000 && file_kind != 0o040000 {
                return Err(format!(
                    "Special filesystem entries are not allowed in plugin bundles: {}",
                    file.name()
                ));
            }
        }

        let raw_name = std::str::from_utf8(file.name_raw())
            .map_err(|_| format!("ZIP entry {index} does not use a UTF-8 path."))?;
        let is_directory = file.is_dir();
        let (relative_path, archive_path) =
            validate_archive_entry_path(raw_name, is_directory, limits.max_path_bytes)?;
        let enclosed_name = file
            .enclosed_name()
            .ok_or_else(|| format!("Unsafe path in plugin bundle: {raw_name}"))?;
        if enclosed_name != relative_path {
            return Err(format!("Ambiguous path in plugin bundle: {raw_name}"));
        }

        let collision_key = archive_path.to_lowercase();
        if !normalized_paths.insert(collision_key) {
            return Err(format!(
                "Plugin bundle contains a duplicate path: {archive_path}"
            ));
        }

        let size = file.size();
        if is_directory && size != 0 {
            return Err(format!(
                "Plugin bundle directory entry contains data: {archive_path}"
            ));
        }
        if !is_directory {
            if size > limits.max_file_bytes {
                return Err(format!(
                    "Plugin bundle file '{archive_path}' is too large ({size} bytes; maximum is {} bytes).",
                    limits.max_file_bytes
                ));
            }
            total_bytes = total_bytes
                .checked_add(size)
                .ok_or_else(|| "Plugin archive size overflowed.".to_string())?;
            if total_bytes > limits.max_total_bytes {
                return Err(format!(
                    "Plugin bundle contents exceed the {} byte limit.",
                    limits.max_total_bytes
                ));
            }
        }

        entries.push(ArchiveEntry {
            index,
            relative_path,
            archive_path,
            size,
            is_directory,
        });
    }

    Ok(entries)
}

fn validate_archive_entry_path(
    raw_name: &str,
    is_directory: bool,
    max_path_bytes: usize,
) -> Result<(PathBuf, String), String> {
    if raw_name.is_empty() || raw_name.len() > max_path_bytes {
        return Err("Plugin bundle contains an empty or overlong path.".to_string());
    }
    if raw_name.contains('\\')
        || raw_name.starts_with('/')
        || raw_name.contains('\0')
        || (!is_directory && raw_name.ends_with('/'))
    {
        return Err(format!("Unsafe path in plugin bundle: {raw_name}"));
    }

    let normalized = if is_directory {
        raw_name
            .strip_suffix('/')
            .ok_or_else(|| format!("Invalid directory path in plugin bundle: {raw_name}"))?
    } else {
        raw_name
    };
    if normalized.is_empty() {
        return Err("The plugin bundle cannot contain a root directory entry.".to_string());
    }

    let mut relative_path = PathBuf::new();
    let mut components = Vec::new();
    for component in normalized.split('/') {
        if !is_safe_archive_component(component) {
            return Err(format!(
                "Plugin bundle path contains an unsafe component: {raw_name}"
            ));
        }
        relative_path.push(component);
        components.push(component);
    }

    Ok((relative_path, components.join("/")))
}

fn portable_relative_path(path: &Path, max_path_bytes: usize) -> Result<String, String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("Plugin bundle paths must be nonempty and relative.".to_string());
    }

    let mut components = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(format!("Unsafe plugin bundle path: {}", path.display()));
        };
        let value = value.to_str().ok_or_else(|| {
            format!(
                "Plugin bundle path is not valid Unicode: {}",
                path.display()
            )
        })?;
        if !is_safe_archive_component(value) {
            return Err(format!(
                "Plugin bundle path contains an unsafe component: {}",
                path.display()
            ));
        }
        components.push(value);
    }

    let portable = components.join("/");
    if portable.len() > max_path_bytes {
        return Err(format!(
            "Plugin bundle path is too long: {}",
            path.display()
        ));
    }
    Ok(portable)
}

fn is_safe_archive_component(component: &str) -> bool {
    if component.is_empty()
        || component == "."
        || component == ".."
        || component.ends_with([' ', '.'])
        || component.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
        })
    {
        return false;
    }

    let uppercase = component.to_ascii_uppercase();
    let stem = uppercase.split('.').next().unwrap_or("");
    !matches!(
        stem,
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn read_bundle_marker(
    archive: &mut ZipArchive<File>,
    entries: &[ArchiveEntry],
    limits: BundleLimits,
) -> Result<BundleMarker, String> {
    let marker_entry = entries
        .iter()
        .find(|entry| entry.archive_path == BUNDLE_MARKER_NAME && !entry.is_directory)
        .ok_or_else(|| {
            format!(
                "This is not a Cultivator plugin bundle; '{}' is missing.",
                BUNDLE_MARKER_NAME
            )
        })?;
    if marker_entry.size > limits.max_marker_bytes {
        return Err(format!(
            "Plugin bundle marker is too large ({} bytes; maximum is {} bytes).",
            marker_entry.size, limits.max_marker_bytes
        ));
    }

    let mut marker_file = archive
        .by_index(marker_entry.index)
        .map_err(|error| format!("Failed to read plugin bundle marker: {error}"))?;
    let marker_bytes = read_limited(
        &mut marker_file,
        limits.max_marker_bytes,
        "plugin bundle marker",
    )?;
    serde_json::from_slice::<BundleMarker>(&marker_bytes)
        .map_err(|error| format!("Failed to parse plugin bundle marker: {error}"))
}

fn read_limited(input: &mut impl Read, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    input
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Failed to read {label}: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{label} exceeds the {limit} byte limit."));
    }
    Ok(bytes)
}

fn validate_bundle_marker(marker: &BundleMarker, entries: &[ArchiveEntry]) -> Result<(), String> {
    if marker.format != BUNDLE_FORMAT {
        return Err(format!(
            "Unsupported plugin bundle format '{}'.",
            marker.format
        ));
    }
    if marker.format_version != BUNDLE_FORMAT_VERSION {
        return Err(format!(
            "Unsupported plugin bundle version {}. Cultivator supports version {}.",
            marker.format_version, BUNDLE_FORMAT_VERSION
        ));
    }
    if marker.plugins.is_empty() {
        return Err("Plugin bundle does not contain any plugins.".to_string());
    }

    let mut plugin_ids = HashSet::new();
    let mut plugin_paths = HashSet::new();
    for plugin in &marker.plugins {
        if !is_safe_plugin_id(&plugin.id) || plugin.id.trim().is_empty() {
            return Err(format!(
                "Plugin bundle contains an invalid plugin id: '{}'.",
                plugin.id
            ));
        }
        let (_, normalized_path) = validate_archive_entry_path(&plugin.path, false, 4_096)?;
        if normalized_path != plugin.path {
            return Err(format!(
                "Plugin bundle contains a non-normalized plugin path: {}",
                plugin.path
            ));
        }
        let relative_path = path_from_portable(&plugin.path);
        if path_uses_reserved_root(&relative_path) || is_export_excluded(&relative_path) {
            return Err(format!(
                "Plugin bundle uses a reserved or non-exportable plugin path: {}",
                plugin.path
            ));
        }
        if !plugin_ids.insert(plugin.id.to_ascii_lowercase()) {
            return Err(format!(
                "Plugin bundle contains duplicate plugin id '{}'.",
                plugin.id
            ));
        }
        if !plugin_paths.insert(plugin.path.to_lowercase()) {
            return Err(format!(
                "Plugin bundle contains duplicate plugin path '{}'.",
                plugin.path
            ));
        }
    }
    reject_overlapping_plugin_paths(&marker.plugins)?;

    let mut support_paths = HashSet::new();
    for support_file in &marker.support_files {
        let (_, normalized_path) = validate_archive_entry_path(support_file, false, 4_096)?;
        if normalized_path != *support_file || support_file == BUNDLE_MARKER_NAME {
            return Err(format!(
                "Plugin bundle contains an invalid support file path: {support_file}"
            ));
        }
        let relative_path = path_from_portable(support_file);
        if path_uses_reserved_root(&relative_path) || is_export_excluded(&relative_path) {
            return Err(format!(
                "Plugin bundle uses a reserved or non-exportable support path: {support_file}"
            ));
        }
        if marker
            .plugins
            .iter()
            .any(|plugin| path_is_equal_or_descendant(support_file, &plugin.path))
        {
            return Err(format!(
                "Support file '{support_file}' is inside a declared plugin directory."
            ));
        }
        if !support_paths.insert(support_file.to_lowercase()) {
            return Err(format!(
                "Plugin bundle contains duplicate support file path '{support_file}'."
            ));
        }
    }

    let entry_map = entries
        .iter()
        .map(|entry| (entry.archive_path.to_lowercase(), entry))
        .collect::<HashMap<_, _>>();
    for support_file in &marker.support_files {
        match entry_map.get(&support_file.to_lowercase()) {
            Some(entry) if !entry.is_directory => {}
            _ => {
                return Err(format!(
                    "Plugin bundle support file is missing: {support_file}"
                ));
            }
        }
    }
    for plugin in &marker.plugins {
        let manifest_path = format!("{}/plugin.toml", plugin.path);
        match entry_map.get(&manifest_path.to_lowercase()) {
            Some(entry) if !entry.is_directory => {}
            _ => {
                return Err(format!(
                    "Plugin bundle manifest is missing: {manifest_path}"
                ));
            }
        }
    }

    for entry in entries {
        if entry.archive_path == BUNDLE_MARKER_NAME {
            continue;
        }
        if path_uses_reserved_root(&entry.relative_path) || is_export_excluded(&entry.relative_path)
        {
            return Err(format!(
                "Plugin bundle contains a reserved or non-exportable entry: {}",
                entry.archive_path
            ));
        }
        let declared_support = support_paths.contains(&entry.archive_path.to_lowercase());
        let declared_plugin_content = marker
            .plugins
            .iter()
            .any(|plugin| path_is_equal_or_descendant(&entry.archive_path, &plugin.path));
        let declared_directory = entry.is_directory
            && (marker.plugins.iter().any(|plugin| {
                path_is_equal_or_descendant(&entry.archive_path, &plugin.path)
                    || path_is_equal_or_descendant(&plugin.path, &entry.archive_path)
            }) || marker
                .support_files
                .iter()
                .any(|support| path_is_equal_or_descendant(support, &entry.archive_path)));

        if !declared_support && !declared_plugin_content && !declared_directory {
            return Err(format!(
                "Plugin bundle contains an undeclared entry: {}",
                entry.archive_path
            ));
        }
    }

    Ok(())
}

fn reject_overlapping_plugin_paths(plugins: &[BundlePluginDescriptor]) -> Result<(), String> {
    for (index, left) in plugins.iter().enumerate() {
        for right in plugins.iter().skip(index + 1) {
            if path_is_equal_or_descendant(&left.path, &right.path)
                || path_is_equal_or_descendant(&right.path, &left.path)
            {
                return Err(format!(
                    "Plugin bundle paths overlap: '{}' and '{}'.",
                    left.path, right.path
                ));
            }
        }
    }
    Ok(())
}

fn path_is_equal_or_descendant(path: &str, parent: &str) -> bool {
    path.eq_ignore_ascii_case(parent)
        || path
            .get(..parent.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(parent))
            && path.as_bytes().get(parent.len()) == Some(&b'/')
}

fn extract_archive_to_staging(
    archive: &mut ZipArchive<File>,
    entries: &[ArchiveEntry],
    staging_root: &Path,
    limits: BundleLimits,
) -> Result<(), String> {
    let mut actual_total = 0u64;
    for entry in entries {
        if entry.archive_path == BUNDLE_MARKER_NAME {
            continue;
        }

        let destination = staging_root.join(&entry.relative_path);
        if entry.is_directory {
            fs::create_dir_all(&destination).map_err(|error| {
                format!(
                    "Failed to create staged plugin directory '{}': {error}",
                    destination.display()
                )
            })?;
            continue;
        }

        let parent = destination.parent().ok_or_else(|| {
            format!(
                "Staged plugin file has no parent directory: {}",
                destination.display()
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "Failed to create staged plugin directory '{}': {error}",
                parent.display()
            )
        })?;
        let mut output = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&destination)
            .map_err(|error| {
                format!(
                    "Failed to create staged plugin file '{}': {error}",
                    destination.display()
                )
            })?;
        let mut input = archive
            .by_index(entry.index)
            .map_err(|error| format!("Failed to read '{}': {error}", entry.archive_path))?;
        let copied = io::copy(
            &mut input.by_ref().take(limits.max_file_bytes.saturating_add(1)),
            &mut output,
        )
        .map_err(|error| format!("Failed to extract '{}': {error}", entry.archive_path))?;
        if copied > limits.max_file_bytes || copied != entry.size {
            return Err(format!(
                "Plugin bundle file '{}' has an invalid extracted size.",
                entry.archive_path
            ));
        }
        actual_total = actual_total
            .checked_add(copied)
            .ok_or_else(|| "Extracted plugin bundle size overflowed.".to_string())?;
        if actual_total > limits.max_total_bytes {
            return Err(format!(
                "Extracted plugin bundle exceeds the {} byte limit.",
                limits.max_total_bytes
            ));
        }
        output.flush().map_err(|error| {
            format!(
                "Failed to flush staged plugin file '{}': {error}",
                destination.display()
            )
        })?;
    }
    Ok(())
}

fn validate_staged_plugins(
    staging_root: &Path,
    marker: &BundleMarker,
) -> Result<Vec<(BundlePluginDescriptor, PythonPluginManifest)>, String> {
    let mut staged_plugins = Vec::with_capacity(marker.plugins.len());
    for descriptor in &marker.plugins {
        let plugin_directory = staging_root.join(path_from_portable(&descriptor.path));
        let manifest_path = plugin_directory.join("plugin.toml");
        let manifest_size = fs::metadata(&manifest_path)
            .map_err(|error| {
                format!(
                    "Failed to inspect staged plugin manifest '{}': {error}",
                    manifest_path.display()
                )
            })?
            .len();
        if manifest_size > MAX_PLUGIN_MANIFEST_BYTES {
            return Err(format!(
                "Staged plugin manifest '{}' exceeds the {} byte limit.",
                manifest_path.display(),
                MAX_PLUGIN_MANIFEST_BYTES
            ));
        }
        let mut manifest_file = File::open(&manifest_path).map_err(|error| {
            format!(
                "Failed to read staged plugin manifest '{}': {error}",
                manifest_path.display()
            )
        })?;
        let manifest_bytes = read_limited(
            &mut manifest_file,
            MAX_PLUGIN_MANIFEST_BYTES,
            "staged plugin manifest",
        )?;
        let manifest_text = String::from_utf8(manifest_bytes).map_err(|_| {
            format!(
                "Staged plugin manifest '{}' is not valid UTF-8.",
                manifest_path.display()
            )
        })?;
        let migrated_manifest_text = add_missing_manifest_metadata(&manifest_text)?;
        let manifest =
            toml::from_str::<PythonPluginManifest>(&migrated_manifest_text).map_err(|error| {
                format!(
                    "Failed to parse staged plugin manifest '{}': {error}",
                    manifest_path.display()
                )
            })?;
        if manifest.id != descriptor.id {
            return Err(format!(
                "Plugin bundle marker id '{}' does not match manifest id '{}'.",
                descriptor.id, manifest.id
            ));
        }
        validate_manifest(&manifest, &plugin_directory)?;

        if migrated_manifest_text != manifest_text {
            fs::write(
                &manifest_path,
                normalize_toml_for_write(&migrated_manifest_text),
            )
            .map_err(|error| {
                format!(
                    "Failed to migrate staged plugin manifest '{}': {error}",
                    manifest_path.display()
                )
            })?;
        }
        staged_plugins.push((descriptor.clone(), manifest));
    }
    Ok(staged_plugins)
}

fn preflight_import(
    plugin_root: &Path,
    staging_root: &Path,
    installed_plugins: &[LoadedPythonPlugin],
    marker: &BundleMarker,
    staged_plugins: &[(BundlePluginDescriptor, PythonPluginManifest)],
) -> Result<(), String> {
    let installed_ids = installed_plugins
        .iter()
        .map(|plugin| plugin.manifest.id.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    for (descriptor, manifest) in staged_plugins {
        if conflicts_with_builtin_id(&manifest.id) {
            return Err(format!(
                "Plugin id '{}' conflicts with a built-in Cultivator plugin.",
                manifest.id
            ));
        }
        if installed_ids.contains(&manifest.id.to_ascii_lowercase()) {
            return Err(format!(
                "Python plugin '{}' is already installed.",
                manifest.id
            ));
        }

        let destination = plugin_root.join(path_from_portable(&descriptor.path));
        if symlink_metadata_if_present(&destination)?.is_some() {
            return Err(format!(
                "Python plugin folder already exists: {}",
                destination.display()
            ));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| format!("Plugin '{}' has an invalid destination.", manifest.id))?;
        ensure_not_inside_plugin_directory(plugin_root, parent)?;
        validate_existing_parent_chain(plugin_root, parent)?;
    }

    for support_file in &marker.support_files {
        let relative_path = path_from_portable(support_file);
        let staged_path = staging_root.join(&relative_path);
        let destination = plugin_root.join(&relative_path);
        let parent = destination
            .parent()
            .ok_or_else(|| format!("Support file '{support_file}' has an invalid destination."))?;
        ensure_not_inside_plugin_directory(plugin_root, parent)?;
        validate_existing_parent_chain(plugin_root, parent)?;

        if let Some(metadata) = symlink_metadata_if_present(&destination)? {
            if !metadata.file_type().is_file()
                || metadata.file_type().is_symlink()
                || !files_are_identical(&staged_path, &destination)?
            {
                return Err(format!(
                    "Shared plugin support file conflicts with an existing file: {}",
                    destination.display()
                ));
            }
        }
    }

    Ok(())
}

fn validate_existing_parent_chain(plugin_root: &Path, parent: &Path) -> Result<(), String> {
    let relative = parent
        .strip_prefix(plugin_root)
        .map_err(|_| "Plugin destination escaped the Python plugin directory.".to_string())?;
    let mut current = plugin_root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let Some(metadata) = symlink_metadata_if_present(&current)? else {
            continue;
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "Plugin destination parent is not a safe directory: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn files_are_identical(left: &Path, right: &Path) -> Result<bool, String> {
    let left_metadata = fs::metadata(left)
        .map_err(|error| format!("Failed to inspect '{}': {error}", left.display()))?;
    let right_metadata = fs::metadata(right)
        .map_err(|error| format!("Failed to inspect '{}': {error}", right.display()))?;
    if left_metadata.len() != right_metadata.len() {
        return Ok(false);
    }

    let mut left_reader = BufReader::new(
        File::open(left)
            .map_err(|error| format!("Failed to open '{}': {error}", left.display()))?,
    );
    let mut right_reader = BufReader::new(
        File::open(right)
            .map_err(|error| format!("Failed to open '{}': {error}", right.display()))?,
    );
    let mut left_buffer = [0u8; 64 * 1024];
    let mut right_buffer = [0u8; 64 * 1024];
    loop {
        let left_read = left_reader
            .read(&mut left_buffer)
            .map_err(|error| format!("Failed to compare support files: {error}"))?;
        let right_read = right_reader
            .read(&mut right_buffer)
            .map_err(|error| format!("Failed to compare support files: {error}"))?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}

fn install_staged_bundle(
    plugin_root: &Path,
    staging_root: &Path,
    marker: &BundleMarker,
    staged_plugins: &[(BundlePluginDescriptor, PythonPluginManifest)],
) -> Result<Vec<ImportedPythonPlugin>, String> {
    install_staged_bundle_with_hook(plugin_root, staging_root, marker, staged_plugins, || Ok(()))
}

fn install_staged_bundle_with_hook(
    plugin_root: &Path,
    staging_root: &Path,
    marker: &BundleMarker,
    staged_plugins: &[(BundlePluginDescriptor, PythonPluginManifest)],
    mut after_mutation: impl FnMut() -> Result<(), String>,
) -> Result<Vec<ImportedPythonPlugin>, String> {
    let mut installed_plugin_directories = Vec::<PathBuf>::new();
    let mut installed_support_files = Vec::<PathBuf>::new();
    let mut created_directories = Vec::<PathBuf>::new();

    let install_result = (|| {
        for support_file in &marker.support_files {
            let relative_path = path_from_portable(support_file);
            let source = staging_root.join(&relative_path);
            let destination = plugin_root.join(&relative_path);
            if let Some(metadata) = symlink_metadata_if_present(&destination)? {
                if metadata.file_type().is_symlink()
                    || !metadata.is_file()
                    || !files_are_identical(&source, &destination)?
                {
                    return Err(format!(
                        "Shared plugin support file changed during import: {}",
                        destination.display()
                    ));
                }
                continue;
            }
            let parent = destination.parent().ok_or_else(|| {
                format!("Support file '{support_file}' has an invalid destination.")
            })?;
            create_missing_directories(plugin_root, parent, &mut created_directories)?;
            copy_file_no_overwrite(&source, &destination)?;
            installed_support_files.push(destination);
            after_mutation()?;
        }

        for (descriptor, manifest) in staged_plugins {
            let relative_path = path_from_portable(&descriptor.path);
            let source = staging_root.join(&relative_path);
            let destination = plugin_root.join(&relative_path);
            let parent = destination
                .parent()
                .ok_or_else(|| format!("Plugin '{}' has an invalid destination.", manifest.id))?;
            create_missing_directories(plugin_root, parent, &mut created_directories)?;
            fs::create_dir(&destination).map_err(|error| {
                format!(
                    "Failed to reserve Python plugin '{}' destination '{}': {error}",
                    manifest.id,
                    destination.display()
                )
            })?;
            installed_plugin_directories.push(destination.clone());
            copy_directory_contents_no_overwrite(&source, &destination)?;
            after_mutation()?;
        }

        Ok(staged_plugins
            .iter()
            .map(|(descriptor, manifest)| ImportedPythonPlugin {
                id: manifest.id.clone(),
                directory: plugin_root
                    .join(path_from_portable(&descriptor.path))
                    .to_string_lossy()
                    .to_string(),
            })
            .collect::<Vec<_>>())
    })();

    match install_result {
        Ok(imported) => Ok(imported),
        Err(error) => {
            let rollback_errors = rollback_install(
                &installed_plugin_directories,
                &installed_support_files,
                &created_directories,
            );
            if rollback_errors.is_empty() {
                Err(error)
            } else {
                Err(format!(
                    "{error} Rollback also reported: {}",
                    rollback_errors.join("; ")
                ))
            }
        }
    }
}

fn create_missing_directories(
    plugin_root: &Path,
    parent: &Path,
    created_directories: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let relative = parent
        .strip_prefix(plugin_root)
        .map_err(|_| "Plugin destination escaped the Python plugin directory.".to_string())?;
    let mut current = plugin_root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if let Some(metadata) = symlink_metadata_if_present(&current)? {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "Plugin destination parent is not a safe directory: {}",
                    current.display()
                ));
            }
            continue;
        }
        fs::create_dir(&current).map_err(|error| {
            format!(
                "Failed to create plugin organization directory '{}': {error}",
                current.display()
            )
        })?;
        created_directories.push(current.clone());
    }
    Ok(())
}

fn copy_directory_contents_no_overwrite(source: &Path, destination: &Path) -> Result<(), String> {
    let mut pending_directories = vec![(source.to_path_buf(), destination.to_path_buf())];
    let mut files = Vec::<(PathBuf, PathBuf)>::new();
    while let Some((source_directory, destination_directory)) = pending_directories.pop() {
        let mut entries = fs::read_dir(&source_directory)
            .map_err(|error| {
                format!(
                    "Failed to read staged plugin directory '{}': {error}",
                    source_directory.display()
                )
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                format!(
                    "Failed to inspect staged plugin directory '{}': {error}",
                    source_directory.display()
                )
            })?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let source_path = entry.path();
            let destination_path = destination_directory.join(entry.file_name());
            let file_type = entry.file_type().map_err(|error| {
                format!(
                    "Failed to inspect staged plugin file '{}': {error}",
                    source_path.display()
                )
            })?;
            if file_type.is_symlink() {
                return Err(format!(
                    "Staged plugin unexpectedly contains a symbolic link: {}",
                    source_path.display()
                ));
            }
            if file_type.is_dir() {
                fs::create_dir(&destination_path).map_err(|error| {
                    format!(
                        "Failed to create plugin directory '{}': {error}",
                        destination_path.display()
                    )
                })?;
                pending_directories.push((source_path, destination_path));
            } else if file_type.is_file() {
                files.push((source_path, destination_path));
            } else {
                return Err(format!(
                    "Staged plugin contains an unsupported filesystem entry: {}",
                    source_path.display()
                ));
            }
        }
    }

    files.sort_by(|(left_source, _), (right_source, _)| {
        let left_is_manifest = left_source == &source.join("plugin.toml");
        let right_is_manifest = right_source == &source.join("plugin.toml");
        left_is_manifest
            .cmp(&right_is_manifest)
            .then_with(|| left_source.cmp(right_source))
    });
    for (source_path, destination_path) in files {
        copy_file_no_overwrite(&source_path, &destination_path)?;
    }
    Ok(())
}

fn copy_file_no_overwrite(source: &Path, destination: &Path) -> Result<(), String> {
    let mut input = BufReader::new(
        File::open(source)
            .map_err(|error| format!("Failed to open '{}': {error}", source.display()))?,
    );
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)
        .map_err(|error| {
            format!(
                "Failed to create shared support file '{}': {error}",
                destination.display()
            )
        })?;
    let mut cleanup = CleanupPath::file(destination.to_path_buf());
    io::copy(&mut input, &mut output).map_err(|error| {
        format!(
            "Failed to install shared support file '{}': {error}",
            destination.display()
        )
    })?;
    output.flush().map_err(|error| {
        format!(
            "Failed to flush shared support file '{}': {error}",
            destination.display()
        )
    })?;
    output.sync_all().map_err(|error| {
        format!(
            "Failed to synchronize shared support file '{}': {error}",
            destination.display()
        )
    })?;
    cleanup.disarm();
    Ok(())
}

fn symlink_metadata_if_present(path: &Path) -> Result<Option<fs::Metadata>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Failed to inspect '{}': {error}", path.display())),
    }
}

fn rollback_install(
    installed_plugin_directories: &[PathBuf],
    installed_support_files: &[PathBuf],
    created_directories: &[PathBuf],
) -> Vec<String> {
    let mut errors = Vec::new();
    for destination in installed_plugin_directories.iter().rev() {
        match fs::remove_dir_all(destination) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                errors.push(format!(
                    "failed to remove '{}': {error}",
                    destination.display()
                ));
            }
        }
    }
    for support_file in installed_support_files.iter().rev() {
        match fs::remove_file(support_file) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                errors.push(format!(
                    "failed to remove '{}': {error}",
                    support_file.display()
                ));
            }
        }
    }
    for directory in created_directories.iter().rev() {
        match fs::remove_dir(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => errors.push(format!(
                "failed to remove directory '{}': {error}",
                directory.display()
            )),
        }
    }
    errors
}

fn path_from_portable(path: &str) -> PathBuf {
    path.split('/').collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{PythonPluginMode, PythonPluginTarget};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "cultivator-plugin-bundle-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default()
        ))
    }

    fn fixture_manifest(id: &str) -> PythonPluginManifest {
        PythonPluginManifest {
            id: id.to_string(),
            name: format!("{id} Plugin"),
            organization_folder: None,
            author: "Cultivator Tests".to_string(),
            version: "1.0.0".to_string(),
            description: "Bundle fixture".to_string(),
            plugin_type: "other".to_string(),
            target: PythonPluginTarget::Other,
            mode: PythonPluginMode::EachFile,
            path_glob: Vec::new(),
            archive_path_glob: Vec::new(),
            path_regex: None,
            entry: "plugin.py".to_string(),
            function: "run".to_string(),
            options: Vec::new(),
        }
    }

    fn write_fixture_plugin(root: &Path, relative: &str, id: &str) -> LoadedPythonPlugin {
        let directory = root.join(path_from_portable(relative));
        fs::create_dir_all(&directory).unwrap();
        let manifest = fixture_manifest(id);
        fs::write(
            directory.join("plugin.toml"),
            format!(
                "id = \"{id}\"\nname = \"{id} Plugin\"\nauthor = \"Tests\"\nversion = \"1.0.0\"\ndescription = \"Fixture\"\ntype = \"other\"\ntarget = \"other\"\nmode = \"each_file\"\nentry = \"plugin.py\"\nfunction = \"run\"\n"
            ),
        )
        .unwrap();
        fs::write(
            directory.join("plugin.py"),
            b"def run(context):\n    return []\n",
        )
        .unwrap();
        LoadedPythonPlugin {
            manifest,
            directory,
        }
    }

    fn fixture_manifest_text(id: &str, entry: &str) -> String {
        format!(
            "id = \"{id}\"\nname = \"{id} Plugin\"\nauthor = \"Tests\"\nversion = \"1.0.0\"\ndescription = \"Fixture\"\ntype = \"other\"\ntarget = \"other\"\nmode = \"each_file\"\nentry = \"{entry}\"\nfunction = \"run\"\n"
        )
    }

    fn write_raw_bundle(
        archive_path: &Path,
        marker: Option<&BundleMarker>,
        marker_bytes: Option<&[u8]>,
        files: &[(&str, &[u8])],
    ) {
        let output = File::create(archive_path).unwrap();
        let mut archive = ZipWriter::new(output);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        archive.start_file(BUNDLE_MARKER_NAME, options).unwrap();
        if let Some(marker) = marker {
            archive
                .write_all(&serde_json::to_vec(marker).unwrap())
                .unwrap();
        } else {
            archive.write_all(marker_bytes.unwrap_or_default()).unwrap();
        }
        for (path, contents) in files {
            archive.start_file(*path, options).unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn round_trip_preserves_plugins_and_shared_support() {
        let root = temporary_root("round-trip");
        let source = root.join("source").join("plugins").join("python");
        let destination = root.join("destination").join("plugins").join("python");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let plugin_a = write_fixture_plugin(&source, "iLEAPP/phonebook", "phonebook");
        let plugin_b = write_fixture_plugin(&source, "iLEAPP/messages", "messages");
        fs::create_dir_all(source.join("iLEAPP").join("_shared")).unwrap();
        fs::write(
            source.join("iLEAPP").join("_shared").join("parser.py"),
            b"SHARED",
        )
        .unwrap();
        fs::write(source.join(ROOT_API_STUB_NAME), b"SEEDED").unwrap();
        fs::create_dir_all(source.join("__pycache__")).unwrap();
        fs::write(source.join("__pycache__").join("cache.pyc"), b"CACHE").unwrap();
        let archive_path = root.join("plugins.zip");

        let exported = export_python_plugins_with_limits(
            &source,
            &[plugin_a, plugin_b],
            ExportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        )
        .unwrap();
        assert_eq!(exported.plugin_count, 2);

        let imported = import_python_plugins_with_limits(
            &destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        )
        .unwrap();
        assert_eq!(imported.plugin_count, 2);
        assert!(destination.join("iLEAPP/phonebook/plugin.toml").is_file());
        assert!(destination.join("iLEAPP/messages/plugin.py").is_file());
        assert_eq!(
            fs::read(destination.join("iLEAPP/_shared/parser.py")).unwrap(),
            b"SHARED"
        );
        assert!(!destination.join(ROOT_API_STUB_NAME).exists());
        assert!(!destination.join("__pycache__").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_zip_slip_without_writing_outside_staging() {
        let root = temporary_root("zip-slip");
        let destination = root.join("destination").join("plugins").join("python");
        fs::create_dir_all(&destination).unwrap();
        let archive_path = root.join("malicious.zip");
        let output = File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(output);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        let marker = BundleMarker {
            format: BUNDLE_FORMAT.to_string(),
            format_version: BUNDLE_FORMAT_VERSION,
            plugins: vec![BundlePluginDescriptor {
                id: "safe".to_string(),
                path: "safe".to_string(),
            }],
            support_files: Vec::new(),
        };
        archive.start_file(BUNDLE_MARKER_NAME, options).unwrap();
        archive
            .write_all(&serde_json::to_vec(&marker).unwrap())
            .unwrap();
        archive.start_file("../escape.py", options).unwrap();
        archive.write_all(b"escape").unwrap();
        archive.finish().unwrap();

        let result = import_python_plugins_with_limits(
            &destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        );
        assert!(result.is_err());
        assert!(!root.join("escape.py").exists());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_allows_identical_support_and_rejects_changed_support_atomically() {
        let root = temporary_root("support-conflict");
        let source = root.join("source").join("plugins").join("python");
        let good_destination = root.join("good").join("plugins").join("python");
        let bad_destination = root.join("bad").join("plugins").join("python");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(good_destination.join("shared")).unwrap();
        fs::create_dir_all(bad_destination.join("shared")).unwrap();
        let plugin = write_fixture_plugin(&source, "org/plugin", "fixture");
        fs::create_dir_all(source.join("shared")).unwrap();
        fs::write(source.join("shared/helper.py"), b"same").unwrap();
        fs::write(good_destination.join("shared/helper.py"), b"same").unwrap();
        fs::write(bad_destination.join("shared/helper.py"), b"different").unwrap();
        let archive_path = root.join("plugins.zip");
        export_python_plugins_with_limits(
            &source,
            &[plugin],
            ExportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        )
        .unwrap();

        import_python_plugins_with_limits(
            &good_destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        )
        .unwrap();
        assert!(good_destination.join("org/plugin/plugin.py").is_file());

        let result = import_python_plugins_with_limits(
            &bad_destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        );
        assert!(result.is_err());
        assert!(!bad_destination.join("org/plugin").exists());
        assert_eq!(
            fs::read(bad_destination.join("shared/helper.py")).unwrap(),
            b"different"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_enforces_streaming_size_limit() {
        let root = temporary_root("size-limit");
        let source = root.join("source").join("plugins").join("python");
        let destination = root.join("destination").join("plugins").join("python");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        let plugin = write_fixture_plugin(&source, "fixture", "fixture");
        fs::write(plugin.directory.join("large.bin"), vec![7u8; 512]).unwrap();
        let archive_path = root.join("plugins.zip");
        export_python_plugins_with_limits(
            &source,
            &[plugin],
            ExportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        )
        .unwrap();
        let limits = BundleLimits {
            max_file_bytes: 128,
            ..BundleLimits::default()
        };

        let result = import_python_plugins_with_limits(
            &destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            limits,
        );
        assert!(result.is_err());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_undeclared_plugin_hidden_as_support() {
        let root = temporary_root("undeclared-plugin");
        let destination = root.join("destination").join("plugins").join("python");
        fs::create_dir_all(&destination).unwrap();
        let archive_path = root.join("plugins.zip");
        let marker = BundleMarker {
            format: BUNDLE_FORMAT.to_string(),
            format_version: BUNDLE_FORMAT_VERSION,
            plugins: vec![BundlePluginDescriptor {
                id: "declared".to_string(),
                path: "declared".to_string(),
            }],
            support_files: vec!["evil/plugin.toml".to_string(), "evil/plugin.py".to_string()],
        };
        let declared_manifest = fixture_manifest_text("declared", "plugin.py");
        let hidden_manifest = fixture_manifest_text("archive-extractor", "plugin.py");
        write_raw_bundle(
            &archive_path,
            Some(&marker),
            None,
            &[
                ("declared/plugin.toml", declared_manifest.as_bytes()),
                ("declared/plugin.py", b"def run(context): return []"),
                ("evil/plugin.toml", hidden_manifest.as_bytes()),
                ("evil/plugin.py", b"def run(context): return []"),
            ],
        );

        let result = import_python_plugins_with_limits(
            &destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        );
        assert!(result.is_err());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());
        assert_no_staging_directories(destination.parent().unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_manifest_entry_outside_plugin_without_partial_install() {
        let root = temporary_root("unsafe-entry");
        let destination = root.join("destination").join("plugins").join("python");
        fs::create_dir_all(&destination).unwrap();
        let archive_path = root.join("plugins.zip");
        let marker = BundleMarker {
            format: BUNDLE_FORMAT.to_string(),
            format_version: BUNDLE_FORMAT_VERSION,
            plugins: vec![BundlePluginDescriptor {
                id: "unsafe-entry".to_string(),
                path: "unsafe-entry".to_string(),
            }],
            support_files: vec!["shared.py".to_string()],
        };
        let manifest = fixture_manifest_text("unsafe-entry", "../shared.py");
        write_raw_bundle(
            &archive_path,
            Some(&marker),
            None,
            &[
                ("unsafe-entry/plugin.toml", manifest.as_bytes()),
                ("shared.py", b"def run(context): return []"),
            ],
        );

        let result = import_python_plugins_with_limits(
            &destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        );
        assert!(result.is_err());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());
        assert_no_staging_directories(destination.parent().unwrap());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_case_insensitive_builtin_and_existing_ids() {
        let root = temporary_root("id-conflicts");
        let builtin_destination = root.join("builtin").join("plugins").join("python");
        let existing_destination = root.join("existing").join("plugins").join("python");
        fs::create_dir_all(&builtin_destination).unwrap();
        fs::create_dir_all(&existing_destination).unwrap();
        let existing_plugin =
            write_fixture_plugin(&existing_destination, "installed", "already-installed");

        for (label, plugin_id, destination, installed) in [
            (
                "builtin",
                "ARCHIVE-EXTRACTOR",
                &builtin_destination,
                Vec::new(),
            ),
            (
                "existing",
                "ALREADY-INSTALLED",
                &existing_destination,
                vec![existing_plugin.clone()],
            ),
        ] {
            let archive_path = root.join(format!("{label}.zip"));
            let marker = BundleMarker {
                format: BUNDLE_FORMAT.to_string(),
                format_version: BUNDLE_FORMAT_VERSION,
                plugins: vec![BundlePluginDescriptor {
                    id: plugin_id.to_string(),
                    path: format!("{label}-incoming"),
                }],
                support_files: Vec::new(),
            };
            let manifest = fixture_manifest_text(plugin_id, "plugin.py");
            let manifest_path = format!("{label}-incoming/plugin.toml");
            let script_path = format!("{label}-incoming/plugin.py");
            write_raw_bundle(
                &archive_path,
                Some(&marker),
                None,
                &[
                    (&manifest_path, manifest.as_bytes()),
                    (&script_path, b"def run(context): return []"),
                ],
            );

            let result = import_python_plugins_with_limits(
                destination,
                &installed,
                ImportPythonPluginsRequest {
                    archive_path: archive_path.to_string_lossy().to_string(),
                },
                BundleLimits::default(),
            );
            assert!(result.is_err());
            assert!(!destination.join(format!("{label}-incoming")).exists());
        }

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_malformed_marker_without_mutating_destination() {
        let root = temporary_root("malformed-marker");
        let destination = root.join("destination").join("plugins").join("python");
        fs::create_dir_all(&destination).unwrap();
        let archive_path = root.join("plugins.zip");
        write_raw_bundle(
            &archive_path,
            None,
            Some(b"{not-json"),
            &[("plugin/plugin.toml", b"invalid")],
        );

        let result = import_python_plugins_with_limits(
            &destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        );
        assert!(result.is_err());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_replaces_existing_archive_but_rejects_plugin_root_destination() {
        let root = temporary_root("export-destination");
        let source = root.join("plugins").join("python");
        fs::create_dir_all(&source).unwrap();
        let plugin = write_fixture_plugin(&source, "fixture", "fixture");
        let archive_path = root.join("plugins.zip");
        fs::write(&archive_path, b"old archive").unwrap();

        export_python_plugins_with_limits(
            &source,
            std::slice::from_ref(&plugin),
            ExportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        )
        .unwrap();
        assert_ne!(fs::read(&archive_path).unwrap(), b"old archive");

        let unsafe_archive = source.join("plugins.zip");
        let result = export_python_plugins_with_limits(
            &source,
            &[plugin],
            ExportPythonPluginsRequest {
                archive_path: unsafe_archive.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        );
        assert!(result.is_err());
        assert!(!unsafe_archive.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_zip_symlinks() {
        let root = temporary_root("zip-symlink");
        let destination = root.join("destination").join("plugins").join("python");
        fs::create_dir_all(&destination).unwrap();
        let archive_path = root.join("plugins.zip");
        let output = File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(output);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        let marker = BundleMarker {
            format: BUNDLE_FORMAT.to_string(),
            format_version: BUNDLE_FORMAT_VERSION,
            plugins: vec![BundlePluginDescriptor {
                id: "fixture".to_string(),
                path: "fixture".to_string(),
            }],
            support_files: Vec::new(),
        };
        archive.start_file(BUNDLE_MARKER_NAME, options).unwrap();
        archive
            .write_all(&serde_json::to_vec(&marker).unwrap())
            .unwrap();
        archive.start_file("fixture/plugin.toml", options).unwrap();
        archive
            .write_all(fixture_manifest_text("fixture", "plugin.py").as_bytes())
            .unwrap();
        archive.start_file("fixture/plugin.py", options).unwrap();
        archive.write_all(b"def run(context): return []").unwrap();
        archive
            .add_symlink("fixture/link", "../../outside", options)
            .unwrap();
        archive.finish().unwrap();

        let result = import_python_plugins_with_limits(
            &destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        );
        assert!(result.is_err());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_rejects_encrypted_entries_before_decompression() {
        let root = temporary_root("zip-encrypted");
        let destination = root.join("destination").join("plugins").join("python");
        fs::create_dir_all(&destination).unwrap();
        let archive_path = root.join("plugins.zip");
        let output = File::create(&archive_path).unwrap();
        let mut archive = ZipWriter::new(output);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644)
            .with_aes_encryption(zip::AesMode::Aes256, "password");
        archive.start_file(BUNDLE_MARKER_NAME, options).unwrap();
        archive.write_all(b"encrypted marker").unwrap();
        archive.finish().unwrap();

        let result = import_python_plugins_with_limits(
            &destination,
            &[],
            ImportPythonPluginsRequest {
                archive_path: archive_path.to_string_lossy().to_string(),
            },
            BundleLimits::default(),
        );
        assert!(result.is_err());
        assert!(fs::read_dir(&destination).unwrap().next().is_none());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn archive_paths_reject_absolute_drive_parent_and_backslash_forms() {
        for path in [
            "../escape.py",
            "/absolute.py",
            "C:/drive.py",
            r"\\server\\share.py",
            r"folder\\escape.py",
        ] {
            assert!(
                validate_archive_entry_path(path, false, 4_096).is_err(),
                "{path} should be rejected"
            );
        }
        assert!(validate_archive_entry_path("safe/folder/plugin.py", false, 4_096).is_ok());
    }

    #[test]
    fn install_rolls_back_after_a_plugin_directory_was_committed() {
        let root = temporary_root("rollback");
        let plugin_root = root.join("plugins").join("python");
        let staging_root = root.join(".python-plugin-import-fixture");
        fs::create_dir_all(&plugin_root).unwrap();
        fs::create_dir_all(&staging_root).unwrap();
        write_fixture_plugin(&staging_root, "iLEAPP/fixture", "fixture");
        let marker = BundleMarker {
            format: BUNDLE_FORMAT.to_string(),
            format_version: BUNDLE_FORMAT_VERSION,
            plugins: vec![BundlePluginDescriptor {
                id: "fixture".to_string(),
                path: "iLEAPP/fixture".to_string(),
            }],
            support_files: Vec::new(),
        };
        let staged_plugins = validate_staged_plugins(&staging_root, &marker).unwrap();
        let mut mutations = 0usize;

        let result = install_staged_bundle_with_hook(
            &plugin_root,
            &staging_root,
            &marker,
            &staged_plugins,
            || {
                mutations += 1;
                Err("simulated commit failure".to_string())
            },
        );
        assert!(result.is_err());
        assert_eq!(mutations, 1);
        assert!(!plugin_root.join("iLEAPP/fixture").exists());
        assert!(!plugin_root.join("iLEAPP").exists());
        assert!(staging_root.join("iLEAPP/fixture/plugin.py").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    fn assert_no_staging_directories(parent: &Path) {
        let has_staging_directory =
            fs::read_dir(parent)
                .unwrap()
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".python-plugin-import-")
                });
        assert!(!has_staging_directory);
    }
}
