//! Opt-in corpus benchmark; runs production code and retains evidence/results.
use super::*;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("//?/")
        .to_lowercase()
}

fn working_copies(roots: &[String], image: bool) -> (u64, u64) {
    if !image {
        return (0, 0);
    }
    let mut pending: Vec<_> = roots.iter().map(PathBuf::from).collect();
    let (mut files, mut bytes) = (0, 0);
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                files += 1;
                bytes += metadata.len();
            }
        }
    }
    (files, bytes)
}

fn verify_hashes(gallery: &MediaGallery, roots: &[String], expected: &HashMap<String, String>) {
    assert_eq!(gallery.photos.len(), expected.len());
    assert_eq!(gallery.videos.len(), 0);
    for item in &gallery.photos {
        let key = normalized(Path::new(&item.path));
        let relative = roots
            .iter()
            .find_map(|root| key.strip_prefix(&(normalized(Path::new(root)) + "/")))
            .unwrap();
        let actual = item
            .metadata
            .iter()
            .find(|field| field.label == "SHA-256")
            .expect("plugin computed SHA-256");
        assert_eq!(
            &actual.value,
            expected
                .get(relative)
                .expect("file exists in independent copy manifest"),
            "{}",
            item.path
        );
    }
}

#[test]
#[ignore = "manual actual E01/native corpus performance benchmark"]
fn benchmark_e01_media_pipeline() {
    let input = std::env::var("CULTIVATOR_E01_BENCH_PATH").expect("image or copied corpus");
    let case = PathBuf::from(
        std::env::var("CULTIVATOR_E01_BENCH_CASE").expect("new benchmark case directory"),
    );
    let manifest =
        std::env::var("CULTIVATOR_E01_BENCH_MANIFEST").expect("independent copy hash manifest");
    let image =
        std::env::var("CULTIVATOR_E01_BENCH_MODE").unwrap_or_else(|_| "image".into()) == "image";
    let python = std::env::var("CULTIVATOR_MEDIA_BENCH_PYTHON_ROOT")
        .expect("existing models and Python environments");
    let samples = std::env::var("CULTIVATOR_E01_BENCH_SAMPLES")
        .map(|value| value.parse::<u32>().expect("positive sample count"))
        .unwrap_or(3);
    assert!((1..=10).contains(&samples));
    fs::create_dir(&case).expect("case must not already exist");
    let corpus: Vec<JsonValue> =
        serde_json::from_reader(fs::File::open(manifest).unwrap()).unwrap();
    let expected: HashMap<_, _> = corpus
        .iter()
        .map(|f| {
            (
                f["path"].as_str().unwrap().to_lowercase(),
                f["sha256"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let total_bytes: u64 = corpus.iter().map(|f| f["bytes"].as_u64().unwrap()).sum();
    let mut results = Vec::new();
    let import_started = Instant::now();
    let roots = if image {
        let imported = crate::evidence::import_for_test(crate::evidence::ImportRequest {
            case_folder_path: case.to_string_lossy().into_owned(),
            datasource_id: "e01-benchmark".into(),
            source_type: "diskImage".into(),
            paths: vec![input.clone()],
            partition: None,
        })
        .unwrap();
        assert_eq!(imported.files as usize, expected.len());
        assert_eq!(imported.bytes, total_bytes);
        imported.paths
    } else {
        vec![input.clone()]
    };
    let import_ms = import_started.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        working_copies(&roots, image),
        (0, 0),
        "import must not extract payloads"
    );
    eprintln!(
        "E01_BENCH import_ms={import_ms:.3} files={} bytes={total_bytes} working_files=0",
        expected.len()
    );
    let save = |results: &Vec<JsonValue>| {
        fs::write(case.join("benchmark-results.json"), serde_json::to_string_pretty(&serde_json::json!({
            "input":input, "mode":if image {"image"} else {"native-copy"}, "profile":"development/test",
            "files": expected.len(), "sourceBytes": total_bytes, "importMs":import_ms,
            "workingRoots":roots, "stages":results,
        })).unwrap()).unwrap();
    };
    save(&results);
    for run in 1..=samples {
        let started = Instant::now();
        let mut first_progress = None;
        let gallery =
            image_metadata::execute_with_cancellation(roots.clone(), false, &|| false, |event| {
                if event.scanned_files > 0 && first_progress.is_none() {
                    first_progress = Some(started.elapsed().as_secs_f64() * 1000.0);
                }
            })
            .unwrap();
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(gallery.photos.len(), expected.len());
        assert_eq!(working_copies(&roots, image), (0, 0));
        eprintln!(
            "E01_BENCH discovery_run={run} ms={elapsed:.3} first_progress_ms={:.3} working_files=0",
            first_progress.unwrap_or_default()
        );
        results.push(serde_json::json!({"stage":"discovery", "run":run, "ms":elapsed, "firstProgressMs":first_progress, "workingFiles":0}));
        save(&results);
        let started = Instant::now();
        let mut metadata = std::collections::BTreeMap::new();
        for item in gallery.photos {
            let mut item = image_metadata::load_metadata(item).unwrap();
            // Evidence provenance is deliberately different from native paths.
            // Compare embedded metadata rather than container/path labels.
            item.metadata.retain(|field| {
                !matches!(field.label.as_str(), "Evidence source" | "Evidence path")
            });
            metadata.insert(item.name, item.metadata);
        }
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(working_copies(&roots, image), (0, 0));
        use sha2::Digest;
        let exif_sha256 = format!(
            "{:x}",
            sha2::Sha256::digest(serde_json::to_vec(&metadata).unwrap())
        );
        let exif_fields: usize = metadata.values().map(Vec::len).sum();
        let photos_with_exif = metadata
            .values()
            .filter(|fields| !fields.is_empty())
            .count();
        eprintln!("E01_BENCH exif_run={run} ms={elapsed:.3} working_files=0 exif_fields={exif_fields} exif_sha256={exif_sha256}");
        results.push(
            serde_json::json!({"stage":"exif", "run":run, "ms":elapsed,"workingFiles":0,
            "exifFields":exif_fields,"photosWithExif":photos_with_exif,"exifSha256":exif_sha256}),
        );
        save(&results);
    }
    if std::env::var("CULTIVATOR_E01_BENCH_METADATA_ONLY").as_deref() == Ok("1") {
        return;
    }
    let started = Instant::now();
    let gallery =
        image_metadata::execute_with_cancellation(roots.clone(), true, &|| false, |_| {}).unwrap();
    let hashing_ms = started.elapsed().as_secs_f64() * 1000.0;
    verify_hashes(&gallery, &roots, &expected);
    assert_eq!(working_copies(&roots, image), (0, 0));
    eprintln!(
        "E01_BENCH discovery_and_hashing_ms={hashing_ms:.3} verified_hashes={} working_files=0",
        expected.len()
    );
    results.push(serde_json::json!({"stage":"discovery-and-hashing","ms":hashing_ms,"verifiedHashes":expected.len(),"workingFiles":0}));
    save(&results);
    let runs = if image { 2 } else { 1 };
    for run in 1..=runs {
        let before = working_copies(&roots, image);
        let mut progress = |_event: BuiltinPluginProgress| {};
        let mut context = BuiltinPluginContext::new(
            roots.clone(),
            None,
            serde_json::json!({
                "hashMedia":true,"screenSkin":true,"groupSimilar":true,"detectFaces":true,
                "classifyClip":true,"classifyNsfw":true,"clipModel":"openclip-vit-b-32",
                "clipThreshold":"0.22","analysisMode":"balanced"
            }),
            vec![],
            &mut progress,
            &|| false,
        );
        context.python_root = Some(python.clone().into());
        let started = Instant::now();
        ImageMetadataPlugin.run(&mut context).unwrap();
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        let effects = context.into_effects();
        for (level, message) in &effects.logs {
            eprintln!("E01_PLUGIN_LOG run={run} {level}: {message}");
        }
        let gallery = effects.media.unwrap();
        verify_hashes(&gallery, &roots, &expected);
        let faces = gallery
            .photos
            .iter()
            .filter(|i| i.face_count.is_some())
            .count();
        let nsfw = gallery
            .photos
            .iter()
            .filter(|i| i.nsfw_score.is_some())
            .count();
        let labeled = gallery
            .photos
            .iter()
            .filter(|i| !i.clip_classifications.is_empty())
            .count();
        assert_eq!(faces, expected.len());
        assert_eq!(nsfw, expected.len());
        let after = working_copies(&roots, image);
        if image {
            assert_eq!(after, (expected.len() as u64, total_bytes));
        }
        eprintln!("E01_BENCH full_run={run} ms={elapsed:.3} working_files_before={} working_files_after={} working_bytes={} faces={faces} nsfw={nsfw} clip_labeled={labeled}", before.0, after.0, after.1);
        results.push(serde_json::json!({"stage":"full-plugin","run":run,"ms":elapsed,"workingFilesBefore":before.0,
            "workingFilesAfter":after.0,"workingBytes":after.1,"facesAnalyzed":faces,"nsfwAnalyzed":nsfw,
            "clipLabeled":labeled,"logs":effects.logs}));
        save(&results);
    }
}
