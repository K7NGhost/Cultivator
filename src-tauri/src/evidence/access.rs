//! Indexed evidence namespace and on-demand extraction, shared by plugins and viewers.
use super::*;
use inodex::filesystem::Metadata;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::sync::mpsc;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IndexedFile {
    pub source: String,
    pub partition: Option<u32>,
    pub evidence_path: String,
    pub local_path: Option<PathBuf>,
    pub metadata: Metadata,
    #[serde(default)]
    source_state: Option<SourceState>,
}
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct SourceState {
    size: u64,
    modified_ns: Option<u128>,
}
pub(super) fn source_state(path: &Path) -> Result<SourceState, String> {
    let metadata = fs::metadata(path).map_err(|e| format!("Evidence source unavailable: {e}"))?;
    Ok(SourceState {
        size: metadata.len(),
        modified_ns: metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|t| t.as_nanos()),
    })
}
fn validate_source(file: &IndexedFile) -> Result<(), String> {
    if let Some(expected) = &file.source_state {
        if source_state(Path::new(&file.source))? != *expected {
            return Err(
                "Evidence container changed since import; reimport it before processing".into(),
            );
        }
    }
    Ok(())
}
struct Index {
    root: PathBuf,
    files: Vec<IndexedFile>,
    by_path: HashMap<String, usize>,
    roots: HashSet<String>,
}
type IndexCache = HashMap<PathBuf, (std::time::SystemTime, u64, Arc<Index>)>;
static INDEXES: OnceLock<Mutex<IndexCache>> = OnceLock::new();
pub(super) fn key(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("//?/")
        .trim_end_matches('/')
        .to_lowercase()
}
fn index(path: &Path) -> Result<Option<Arc<Index>>, String> {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Evidence paths cannot contain parent traversal".into());
    }
    let Some(root) = path.ancestors().find(|p| {
        p.parent()
            .and_then(Path::file_name)
            .is_some_and(|n| n == "evidence")
            && p.join("source.json").is_file()
            && p.join("manifest.ndjson").is_file()
    }) else {
        return Ok(None);
    };
    let manifest = root.join("manifest.ndjson");
    let stat = fs::metadata(&manifest).map_err(|e| e.to_string())?;
    let modified = stat.modified().map_err(|e| e.to_string())?;
    let mut cache = INDEXES
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    if let Some((time, size, value)) = cache.get(&manifest) {
        if *time == modified && *size == stat.len() {
            return Ok(Some(value.clone()));
        }
    }
    let description: serde_json::Value =
        serde_json::from_reader(File::open(root.join("source.json")).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let roots = description["paths"]
        .as_array()
        .ok_or("Missing evidence roots")?
        .iter()
        .filter_map(|p| p.as_str())
        .map(|p| key(Path::new(p)))
        .collect();
    let mut files = Vec::new();
    let mut by_path = HashMap::new();
    for line in BufReader::new(File::open(manifest.clone()).map_err(|e| e.to_string())?).lines() {
        let entry: IndexedFile =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if let Some(local) = &entry.local_path {
            let local_key = key(local);
            if !local_key.starts_with(&format!("{}/", key(root))) {
                // Logical sources use their existing paths and the native-file route.
                continue;
            }
            by_path.insert(local_key, files.len());
            files.push(entry);
        }
    }
    let result = Arc::new(Index {
        root: root.to_path_buf(),
        files,
        by_path,
        roots,
    });
    if cache.len() >= 4 {
        cache.clear();
    }
    cache.insert(manifest, (modified, stat.len(), result.clone()));
    Ok(Some(result))
}
pub(crate) fn indexed_metadata(path: &Path) -> Result<Option<IndexedFile>, String> {
    let Some(index) = index(path)? else {
        return Ok(None);
    };
    let path_key = key(path);
    if let Some(n) = index.by_path.get(&path_key) {
        return Ok(Some(index.files[*n].clone()));
    }
    if index.roots.contains(&path_key) {
        let mut root = index
            .files
            .first()
            .cloned()
            .ok_or("Empty evidence filesystem")?;
        root.local_path = Some(path.to_path_buf());
        root.evidence_path = "/".into();
        root.metadata.kind = FileKind::Directory;
        root.metadata.size = 0;
        return Ok(Some(root));
    }
    Err(format!(
        "File is not in the evidence index: {}",
        path.display()
    ))
}
pub(crate) fn indexed_entries(
    path: &Path,
    recursive: bool,
) -> Result<Option<Vec<IndexedFile>>, String> {
    let Some(index) = index(path)? else {
        return Ok(None);
    };
    let path_key = key(path);
    if let Some(n) = index.by_path.get(&path_key) {
        if index.files[*n].metadata.kind == FileKind::File {
            return Ok(Some(vec![index.files[*n].clone()]));
        }
    }
    let prefix = format!("{path_key}/");
    Ok(Some(
        index
            .files
            .iter()
            .filter(|entry| {
                entry.local_path.as_ref().is_some_and(|p| {
                    let k = key(p);
                    k.starts_with(&prefix) && (recursive || !k[prefix.len()..].contains('/'))
                })
            })
            .cloned()
            .collect(),
    ))
}
pub(crate) fn visit_files(
    root: &Path,
    mut visit: impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    if let Some(entries) = indexed_entries(root, true)? {
        for file in entries {
            if file.metadata.kind == FileKind::File {
                visit(file.local_path.as_ref().unwrap())?;
            }
        }
    } else {
        let walker = ignore::WalkBuilder::new(root)
            .hidden(false)
            .ignore(false)
            .parents(false)
            .git_global(false)
            .git_ignore(false)
            .git_exclude(false)
            .follow_links(false)
            .build();
        for entry in walker {
            let entry = entry.map_err(|e| e.to_string())?;
            if entry.file_type().is_some_and(|t| t.is_file()) {
                visit(entry.path())?;
            }
        }
    }
    Ok(())
}

/// Original image containers represented by this indexed datasource root.
pub(crate) fn source_images(root: &Path) -> Result<Vec<PathBuf>, String> {
    let Some(index) = index(root)? else { return Ok(Vec::new()); };
    // Empty filesystems can still contain recoverable logs in raw image space.
    let description: serde_json::Value = serde_json::from_reader(
        File::open(index.root.join("source.json")).map_err(|e| e.to_string())?
    ).map_err(|e| e.to_string())?;
    Ok(description["originalPaths"].as_array().ok_or("Missing original evidence paths")?
        .iter().filter_map(|path| path.as_str().map(PathBuf::from))
        .collect::<std::collections::BTreeSet<_>>().into_iter().collect())
}

// One actor owns each borrowing image/filesystem pair; independent sources can
// run concurrently. An idle actor closes its handles after 15 seconds.
type Response = mpsc::Sender<Result<Vec<u8>, String>>;
enum Operation {
    Prefix(IndexedFile, u64, usize, Response),
    Extract(IndexedFile, PathBuf, Arc<AtomicBool>, Response),
}
static READERS: OnceLock<Mutex<HashMap<String, mpsc::SyncSender<Operation>>>> = OnceLock::new();
fn with_filesystem<T>(
    file: &IndexedFile,
    action: impl FnOnce(&mut FileSystem<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let mut image = Image::open(&file.source).map_err(|e| format!("Open {}: {e}", file.source))?;
    if let Some(number) = file.partition {
        let table = PartitionTable::read(&mut image)
            .map_err(|e| e.to_string())?
            .ok_or("Evidence partition table changed")?;
        let mut volume = table
            .get(number)
            .map_err(|e| e.to_string())?
            .volume(&mut image)
            .map_err(|e| e.to_string())?;
        action(&mut FileSystem::with_limits(&mut volume, limits()).map_err(|e| e.to_string())?)
    } else {
        action(&mut FileSystem::with_limits(&mut image, limits()).map_err(|e| e.to_string())?)
    }
}
fn reader(file: &IndexedFile) -> Result<mpsc::SyncSender<Operation>, String> {
    let source_key = format!("{}:{:?}", file.source, file.partition);
    let mut readers = READERS
        .get_or_init(Default::default)
        .lock()
        .map_err(|e| e.to_string())?;
    if let Some(sender) = readers.get(&source_key) {
        return Ok(sender.clone());
    }
    let (sender, receiver) = mpsc::sync_channel(8);
    let source = file.clone();
    std::thread::spawn(move || {
        let result = with_filesystem(&source, |filesystem| {
            while let Ok(operation) = receiver.recv_timeout(Duration::from_secs(15)) {
                let (entry, count, offset, destination, response) = match operation {
                    Operation::Prefix(file, offset, n, response) => {
                        (file, n, offset, None, response)
                    }
                    Operation::Extract(file, path, cancelled, response) => {
                        (file, 0, 0, Some((path, cancelled)), response)
                    }
                };
                let result = (|| {
                    let id = filesystem
                        .resolve(&entry.evidence_path)
                        .map_err(|e| e.to_string())?;
                    let actual = filesystem.metadata(id).map_err(|e| e.to_string())?;
                    if actual.size != entry.metadata.size || actual.id != entry.metadata.id {
                        return Err("Evidence changed since import; reimport the source".into());
                    }
                    if let Some((path, cancelled)) = destination {
                        let mut output = fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(&path)
                            .map_err(|e| e.to_string())?;
                        let mut writer = CheckedWriter {
                            file: &mut output,
                            cancelled: &cancelled,
                        };
                        filesystem
                            .copy_file(id, None, &mut writer)
                            .map_err(|e| e.to_string())?;
                        output.flush().map_err(|e| e.to_string())?;
                        if let Some(time) = actual.times.modified.filter(|t| {
                            t.basis == inodex::filesystem::TimeBasis::Utc
                                && t.seconds > -11_644_473_600
                        }) {
                            let time = std::time::UNIX_EPOCH
                                .checked_add(Duration::from_secs(time.seconds.max(0) as u64));
                            if actual.times.modified.unwrap().seconds >= 0 {
                                if let Some(time) = time {
                                    output
                                        .set_times(fs::FileTimes::new().set_modified(time))
                                        .map_err(|e| e.to_string())?;
                                }
                            }
                        }
                        Ok(Vec::new())
                    } else {
                        let mut bytes = vec![
                            0;
                            count.min(
                                actual.size.saturating_sub(offset).min(usize::MAX as u64) as usize
                            )
                        ];
                        let mut total = 0;
                        while total < bytes.len() {
                            let n = filesystem
                                .read_file(id, None, offset + total as u64, &mut bytes[total..])
                                .map_err(|e| e.to_string())?;
                            if n == 0 {
                                break;
                            }
                            total += n;
                        }
                        bytes.truncate(total);
                        Ok(bytes)
                    }
                })();
                let _ = response.send(result);
            }
            Ok(())
        });
        if let Err(error) = result {
            while let Ok(operation) = receiver.recv_timeout(Duration::from_millis(100)) {
                let response = match operation {
                    Operation::Prefix(_, _, _, r) | Operation::Extract(_, _, _, r) => r,
                };
                let _ = response.send(Err(error.clone()));
            }
        }
    });
    if readers.len() >= 8 {
        readers.clear();
    }
    readers.insert(source_key, sender.clone());
    Ok(sender)
}
fn request(
    file: &IndexedFile,
    build: impl Fn(Response) -> Operation,
    cancel: Option<(&(dyn Fn() -> bool + Sync), &AtomicBool)>,
) -> Result<Vec<u8>, String> {
    validate_source(file)?;
    for _ in 0..2 {
        let (sender, receiver) = mpsc::channel();
        if reader(file)?.send(build(sender)).is_ok() {
            loop {
                if let Some((external, proxy)) = cancel {
                    proxy.store(external(), Ordering::Relaxed);
                }
                match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(result) => return result,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
        READERS
            .get_or_init(Default::default)
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&format!("{}:{:?}", file.source, file.partition));
    }
    Err("Evidence reader stopped".into())
}
struct CheckedWriter<'a> {
    file: &'a mut File,
    cancelled: &'a AtomicBool,
}
impl Write for CheckedWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("Evidence extraction cancelled"));
        }
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
pub(crate) fn read_prefix(path: &Path, count: usize) -> Result<Vec<u8>, String> {
    if let Some(file) = indexed_metadata(path)? {
        return request(
            &file,
            |reply| Operation::Prefix(file.clone(), 0, count, reply),
            None,
        );
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(count as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
static EXTRACTIONS: OnceLock<Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>> = OnceLock::new();
pub(crate) fn ensure_local(path: &Path, cancelled: &AtomicBool) -> Result<PathBuf, String> {
    ensure_local_checked(path, &|| cancelled.load(Ordering::Relaxed))
}
pub(crate) fn ensure_local_checked(
    path: &Path,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<PathBuf, String> {
    let Some(file) = indexed_metadata(path)? else {
        return Ok(path.to_path_buf());
    };
    if file.metadata.kind != FileKind::File {
        return Err("Select an evidence file to extract".into());
    }
    validate_source(&file)?;
    let lock = {
        let mut locks = EXTRACTIONS
            .get_or_init(Default::default)
            .lock()
            .map_err(|e| e.to_string())?;
        if locks.len() > 4096 {
            locks.retain(|_, value| value.strong_count() > 0);
        }
        let slot = locks.entry(key(path)).or_default();
        let value = slot.upgrade().unwrap_or_else(|| Arc::new(Mutex::new(())));
        *slot = Arc::downgrade(&value);
        value
    };
    let _guard = lock.lock().map_err(|e| e.to_string())?;
    if cancelled() {
        return Err("Evidence extraction cancelled".into());
    }
    let local = file
        .local_path
        .as_ref()
        .ok_or("Evidence has no working path")?;
    if let Ok(metadata) = fs::metadata(local) {
        if metadata.is_file() && metadata.len() == file.metadata.size {
            return Ok(local.clone());
        }
        return Err("Working copy differs from indexed evidence; remove it before retrying".into());
    }
    let parent = local.parent().ok_or("Missing working directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let root = index(path)?
        .ok_or("Missing evidence index")?
        .root
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !parent
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(root)
    {
        return Err("Working path escaped the case directory".into());
    }
    let temporary = parent.join(format!(
        ".cultivator-{}-{}.partial",
        std::process::id(),
        super::unique_id()
    ));
    let cancellation = Arc::new(AtomicBool::new(cancelled()));
    let extract = request(
        &file,
        |reply| Operation::Extract(file.clone(), temporary.clone(), cancellation.clone(), reply),
        Some((cancelled, &cancellation)),
    );
    if extract.is_err() || cancelled() {
        let _ = fs::remove_file(&temporary);
        return Err(extract
            .err()
            .unwrap_or_else(|| "Evidence extraction cancelled".into()));
    }
    fs::rename(&temporary, local).map_err(|e| {
        let _ = fs::remove_file(&temporary);
        e.to_string()
    })?;
    Ok(local.clone())
}
pub(crate) fn ensure_family(path: &Path, cancelled: &AtomicBool) -> Result<PathBuf, String> {
    ensure_family_checked(path, &|| cancelled.load(Ordering::Relaxed))
}
pub(crate) fn ensure_family_checked(
    path: &Path,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<PathBuf, String> {
    let local = ensure_local_checked(path, cancelled)?;
    if let Some(parent) = path.parent() {
        if let Some(entries) = indexed_entries(parent, false)? {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            for entry in entries {
                let Some(sibling) = entry.local_path else {
                    continue;
                };
                let sibling_name = sibling.file_name().unwrap_or_default().to_string_lossy();
                if ["-wal", "-shm", "-journal"]
                    .iter()
                    .any(|suffix| sibling_name.eq_ignore_ascii_case(&format!("{name}{suffix}")))
                {
                    ensure_local_checked(&sibling, cancelled)?;
                }
            }
        }
    }
    Ok(local)
}

pub(crate) enum EvidenceReader {
    Local(File),
    Indexed(IndexedFile, u64),
}
impl EvidenceReader {
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        match indexed_metadata(path)? {
            Some(file) => Ok(Self::Indexed(file, 0)),
            None => File::open(path).map(Self::Local).map_err(|e| e.to_string()),
        }
    }
}
impl Read for EvidenceReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Local(file) => file.read(output),
            Self::Indexed(file, offset) => {
                let bytes = request(
                    file,
                    |reply| Operation::Prefix(file.clone(), *offset, output.len(), reply),
                    None,
                )
                .map_err(std::io::Error::other)?;
                output[..bytes.len()].copy_from_slice(&bytes);
                *offset += bytes.len() as u64;
                Ok(bytes.len())
            }
        }
    }
}
impl Seek for EvidenceReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::Local(file) => file.seek(position),
            Self::Indexed(file, offset) => {
                let target = match position {
                    SeekFrom::Start(n) => n as i128,
                    SeekFrom::Current(n) => *offset as i128 + n as i128,
                    SeekFrom::End(n) => file.metadata.size as i128 + n as i128,
                };
                *offset = u64::try_from(target)
                    .map_err(|_| std::io::Error::other("Invalid evidence seek"))?;
                Ok(*offset)
            }
        }
    }
}
