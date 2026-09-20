use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read embedded frontend directory") {
        let path = entry.expect("read embedded frontend entry").path();
        if path.is_dir() {
            collect_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn main() {
    let root = Path::new("web/dist");
    println!("cargo:rerun-if-changed={}", root.display());

    let mut files = Vec::new();
    collect_files(root, &mut files);
    files.sort();

    let mut fingerprint = DefaultHasher::new();
    for path in files {
        path.strip_prefix(root)
            .expect("frontend asset is below web/dist")
            .hash(&mut fingerprint);
        fs::read(&path)
            .expect("read embedded frontend asset")
            .hash(&mut fingerprint);
    }
    println!(
        "cargo:rustc-env=AGENT_TEACHER_WEB_ASSET_DIGEST={:016x}",
        fingerprint.finish()
    );
}
