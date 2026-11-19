#[path = "src/archive_profile.rs"]
mod archive_profile;

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PLAYBACK_DIR: &str = "GOODHAND_PLAYBACK_DIR";
const TONE_PROFILE: &str = "GOODHAND_TONE_PROFILE";

// The W5 controls were reviewed against this exact embedded W4 pair.
// Compare the bytes retained for embedding, not a second read of the path.
const W4_SHAPE: &str = "c76e1e477d118543f75eed4b63903464acbc6bd1bcf5363d300072afbaedaa19";
const W4_TONE: &str = "5f545d4ba4fce192d9143a0e710932da6a94c2c84e799d3371428d85f0df2739";

fn check_w4_digest(bytes: &[u8], expected: &str, name: &str) -> io::Result<()> {
    let mut child = Command::new("sha256sum")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|error| io::Error::other(format!("W5 builds require sha256sum: {error}")))?;
    let written = child.stdin.take().expect("piped input").write_all(bytes);
    let result = child.wait_with_output()?;
    written?;
    if !result.status.success()
        || result
            .stdout
            .split(|byte| byte.is_ascii_whitespace())
            .next()
            != Some(expected.as_bytes())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("W5 requires the exact reviewed W4 archive; {name} does not match"),
        ));
    }
    Ok(())
}

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
    println!("cargo:rustc-check-cfg=cfg(goodhand_w5_profile)");
    println!("cargo:rerun-if-env-changed={TONE_PROFILE}");
    println!("cargo:rerun-if-env-changed={PLAYBACK_DIR}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/archive_profile.rs");
    let w5 = match env::var_os(TONE_PROFILE) {
        None => false,
        Some(profile) if profile == "w5" => true,
        Some(_) => return Err(format!("{TONE_PROFILE} must be w5 or unset").into()),
    };
    let Some(source) = env::var_os(PLAYBACK_DIR) else {
        if w5 {
            return Err(
                format!("W5 requires {PLAYBACK_DIR} pointing to the reviewed W4 archive").into(),
            );
        }
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
    if w5 {
        check_w4_digest(&shape, W4_SHAPE, "shape.ghs")?;
        check_w4_digest(&tone, W4_TONE, "tone.ght")?;
        println!("cargo:rustc-cfg=goodhand_w5_profile");
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not provide OUT_DIR")?);
    fs::write(output.join("shape.ghs"), shape)?;
    fs::write(output.join("tone.ght"), tone)?;
    println!("cargo:rustc-cfg=goodhand_embedded_playback");
    Ok(())
}
