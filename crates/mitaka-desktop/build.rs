use sha2::{Digest, Sha256};
use std::{fs, path::Path};
fn main() {
    let sums = include_str!("../mitaka-media-web/SHA256SUMS");
    println!("cargo:rerun-if-changed=../mitaka-media-web/SHA256SUMS");
    for line in sums.lines() {
        let (hash, name) = line.split_once("  ").expect("SHA256SUMS format");
        let path = Path::new("src/bin/mitaka-media/web").join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(&path).expect("bundled media bindings");
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            hash,
            "media binding mismatch: {name}"
        );
    }
}
