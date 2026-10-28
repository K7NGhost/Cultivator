use super::*;
use serde_json::json;
use sha2::{Digest, Sha256};

fn log(first_tick: u32) -> Vec<u8> {
    let mut bytes = vec![0; 256];
    bytes[0] = 16;
    bytes[16..22].copy_from_slice(b"BUILD ");
    bytes[242..252].copy_from_slice(b"DJI_LOG_V3");
    for tick in first_tick..first_tick + 5 {
        bytes.extend(dat::fixture_packet(2048, tick, &[0; 120]));
    }
    bytes
}

/// A FAT12 image with one allocated DAT and one log only in unallocated space.
fn image_bytes(allocated: bool) -> Vec<u8> {
    let mut image = vec![0; 2880 * 512];
    image[..3].copy_from_slice(&[0xeb, 0x3c, 0x90]);
    image[11..13].copy_from_slice(&512u16.to_le_bytes());
    image[13] = 1;
    image[14..16].copy_from_slice(&1u16.to_le_bytes());
    image[16] = 2;
    image[17..19].copy_from_slice(&224u16.to_le_bytes());
    image[19..21].copy_from_slice(&2880u16.to_le_bytes());
    image[21] = 0xf0;
    image[22..24].copy_from_slice(&9u16.to_le_bytes());
    image[510..512].copy_from_slice(&[0x55, 0xaa]);
    for start in [512, 5120] {
        image[start..start + 3].copy_from_slice(&[0xf0, 0xff, 0xff]);
        if allocated {
            image[start + 3..start + 6].copy_from_slice(&[3, 0xf0, 0xff]);
        }
    }
    let first = log(1);
    if allocated {
        let entry = 19 * 512;
        image[entry..entry + 11].copy_from_slice(b"FLY000  DAT");
        image[entry + 11] = 0x20;
        image[entry + 26..entry + 28].copy_from_slice(&2u16.to_le_bytes());
        image[entry + 28..entry + 32].copy_from_slice(&(first.len() as u32).to_le_bytes());
    }
    image[33 * 512..33 * 512 + first.len()].copy_from_slice(&first);
    let second = log(100);
    image[0x20000..0x20000 + second.len()].copy_from_slice(&second);
    image
}

fn run(context_paths: Vec<String>, output: &Path, carve: bool) -> Vec<BuiltinArtifact> {
    let mut progress = |_| {};
    let mut context = BuiltinPluginContext::new(
        context_paths,
        Some(output.to_path_buf()),
        json!({"carveDat": carve}),
        vec![],
        &mut progress,
        &|| false,
    );
    super::super::plugin("drone-telemetry")
        .unwrap()
        .run(&mut context)
        .unwrap();
    context.into_effects().artifacts.unwrap()
}

#[test]
fn optional_carving_recovers_unallocated_log_deduplicates_live_log_and_retains_offsets() {
    let case = tempfile::tempdir().unwrap();
    let source = case.path().join("drone.img");
    let image = image_bytes(true);
    fs::write(&source, &image).unwrap();
    let original_hash = Sha256::digest(&image);
    let imported = crate::evidence::import_for_test(crate::evidence::ImportRequest {
        case_folder_path: case.path().to_string_lossy().into_owned(),
        datasource_id: "drone-carve".into(),
        source_type: "diskImage".into(),
        paths: vec![source.to_string_lossy().into_owned()],
        partition: None,
    })
    .unwrap();
    let output = case.path().join("artifacts/drone/test");
    assert_eq!(run(imported.paths.clone(), &output, false).len(), 1);
    let artifacts = run(imported.paths, &output, true);
    assert_eq!(artifacts.len(), 2, "allocated file must not appear twice");
    let carved = artifacts
        .iter()
        .find(|artifact| !artifact.payload["metadata"]["carving"].is_null())
        .unwrap();
    assert_eq!(
        carved.payload["metadata"]["carving"]["imageOffset"],
        0x20000
    );
    assert_eq!(
        carved.payload["metadata"]["carving"]["imagePath"],
        source.to_string_lossy().as_ref()
    );
    assert!(
        !Path::new(&carved.file_path).exists(),
        "a range-backed carve must not be copied upfront"
    );
    let metadata = &carved.payload["metadata"];
    assert_eq!(metadata["csvPending"], true);
    assert!(!Path::new(metadata["csvPath"].as_str().unwrap()).exists());
    let root = Path::new(metadata["csvPath"].as_str().unwrap())
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let restored = dat::restore_file(root, metadata["id"].as_str().unwrap()).unwrap();
    assert!(restored.metadata.csv_pending);
    assert!(!restored.csv_path().exists());
    assert_eq!(
        restored.metadata.source_sha256,
        format!("{:x}", Sha256::digest(log(100)))
    );
    assert_eq!(restored.metadata.carving.unwrap().image_offset, 0x20000);
    assert_eq!(Sha256::digest(fs::read(source).unwrap()), original_hash);
}

#[test]
fn carving_also_works_when_filesystem_index_contains_no_files() {
    let case = tempfile::tempdir().unwrap();
    let source = case.path().join("empty-filesystem.img");
    fs::write(&source, image_bytes(false)).unwrap();
    let imported = crate::evidence::import_for_test(crate::evidence::ImportRequest {
        case_folder_path: case.path().to_string_lossy().into_owned(),
        datasource_id: "empty-drone".into(),
        source_type: "diskImage".into(),
        paths: vec![source.to_string_lossy().into_owned()],
        partition: None,
    })
    .unwrap();
    assert_eq!(imported.files, 0);
    assert_eq!(
        run(imported.paths, &case.path().join("output"), true).len(),
        2
    );
}
