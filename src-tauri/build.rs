fn main() {
    embed_mobile_sources();
    tauri_build::build()
}

fn embed_mobile_sources() {
    use std::{env, fs, path::Path};
    fn collect(root: &Path, directory: &Path, files: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(directory).expect("read bundled mobile sources") {
            let path = entry.expect("mobile source entry").path();
            if path.is_dir() {
                collect(root, &path, files);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");
                assert!(!name.contains("__pycache__") && !name.ends_with(".pyc"));
                files.push((name, path.to_str().unwrap().to_owned()));
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/mobile");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut generated = String::new();
    for (platform, constant) in [
        ("ios", "IOS_SOURCES"),
        ("android", "ANDROID_SOURCES"),
        ("vehicle", "VEHICLE_SOURCES"),
    ] {
        let source = root.join(platform);
        let mut files = Vec::new();
        collect(&source, &source, &mut files);
        files.sort();
        generated.push_str(&format!("static {constant}: &[(&str, &[u8])] = &[\n"));
        for (relative, absolute) in files {
            generated.push_str(&format!("({relative:?}, include_bytes!({absolute:?})),\n"));
        }
        generated.push_str("];\n");
    }
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("mobile_sources.rs"),
        generated,
    )
    .expect("embed mobile sources");
}
