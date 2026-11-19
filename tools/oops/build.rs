#[path = "src/archive_profile.rs"]
mod archive_profile;

use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const PLAYBACK_DIR: &str = "GOODHAND_PLAYBACK_DIR";

fn read_asset(path: &Path) -> io::Result<Vec<u8>> {
    const LIMIT: u64 = goodhand::sequence::MAX_ASSET_BYTES as u64;
    if !path.metadata()?.is_file() {
        return Err(io::Error::other("archive input must be a regular file"));
    }
    let mut bytes = Vec::new();
    File::open(path)?.take(LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::other("archive input exceeds the asset limit"));
    }
    Ok(bytes)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rustc-check-cfg=cfg(goodhand_embedded_playback)");
    println!("cargo:rerun-if-env-changed={PLAYBACK_DIR}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/archive_profile.rs");
    let Some(source) = env::var_os(PLAYBACK_DIR) else {
        return Ok(());
    };
    let source = PathBuf::from(source);
    if !source.is_absolute() {
        return Err(format!("{PLAYBACK_DIR} must name an absolute archive directory").into());
    }
    let input = |name: &str| -> io::Result<Vec<u8>> {
        let path = source.join(name);
        let display = path
            .to_str()
            .ok_or_else(|| io::Error::other("archive path must be UTF-8"))?;
        if display.contains(['\r', '\n']) {
            return Err(io::Error::other("archive path cannot contain a line break"));
        }
        println!("cargo:rerun-if-changed={display}");
        read_asset(&path).map_err(|error| io::Error::other(format!("{display}: {error}")))
    };
    let shape = input("shape.ghs")?;
    let tone = input("tone.ght")?;
    archive_profile::validate(&shape, &tone)?;
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not provide OUT_DIR")?);
    fs::write(output.join("shape.ghs"), shape)?;
    fs::write(output.join("tone.ght"), tone)?;
    println!("cargo:rustc-cfg=goodhand_embedded_playback");
    Ok(())
}
