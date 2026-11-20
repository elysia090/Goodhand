#[path = "src/archive_profile.rs"]
mod archive_profile;

use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PLAYBACK_DIR: &str = "GOODHAND_PLAYBACK_DIR";
const TONE_PROFILE: &str = "GOODHAND_TONE_PROFILE";
const NATIVE_MOTION_DIR: &str = "GOODHAND_NATIVE_MOTION_DIR";

// Only the approved motion pair is compatible with the reviewed W5 build.
// Verify the retained bytes before embedding either private input.
const MOTION_SHAPE: &str = "e44b22610dd3a02024181fd1b856cc1e33c15f13821e72ddda5d24784dffa6ff";
const MOTION_TONE: &str = "52e71a2731a14902ba2bb8db25d141f4f619aa6b7b584eb2be1e47c9550f46e0";

// The W5 controls were reviewed against this exact embedded W4 pair.
// Compare the bytes retained for embedding, not a second read of the path.
const W4_SHAPE: &str = "c76e1e477d118543f75eed4b63903464acbc6bd1bcf5363d300072afbaedaa19";
const W4_TONE: &str = "5f545d4ba4fce192d9143a0e710932da6a94c2c84e799d3371428d85f0df2739";

fn check_digest(bytes: &[u8], expected: &str, label: &str, name: &str) -> io::Result<()> {
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
            format!("{label}; {name} does not match"),
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

fn input(source: &Path, setting: &str, name: &str) -> io::Result<Vec<u8>> {
    if !source.is_absolute() {
        return Err(io::Error::other(format!(
            "{setting} must name an absolute archive directory"
        )));
    }
    let path = source.join(name);
    let display = path
        .to_str()
        .ok_or_else(|| io::Error::other("archive path must be UTF-8"))?;
    if display.contains(['\r', '\n']) {
        return Err(io::Error::other("archive path cannot contain a line break"));
    }
    println!("cargo:rerun-if-changed={display}");
    read_asset(&path).map_err(|error| io::Error::other(format!("{display}: {error}")))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rustc-check-cfg=cfg(goodhand_embedded_playback)");
    println!("cargo:rustc-check-cfg=cfg(goodhand_w5_profile)");
    println!("cargo:rustc-check-cfg=cfg(goodhand_native_motion)");
    println!("cargo:rerun-if-env-changed={NATIVE_MOTION_DIR}");
    println!("cargo:rerun-if-env-changed={TONE_PROFILE}");
    println!("cargo:rerun-if-env-changed={PLAYBACK_DIR}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/archive_profile.rs");
    let w5 = match env::var_os(TONE_PROFILE) {
        None => false,
        Some(profile) if profile == "w5" => true,
        Some(_) => return Err(format!("{TONE_PROFILE} must be w5 or unset").into()),
    };
    let native_motion = env::var_os(NATIVE_MOTION_DIR).map(PathBuf::from);
    let desktop = env::var_os("CARGO_FEATURE_DESKTOP_WINDOW").is_some()
        && env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux");
    if native_motion.is_some() && (!w5 || !desktop) {
        return Err(format!(
            "{NATIVE_MOTION_DIR} requires W5 and the Linux desktop-window feature"
        )
        .into());
    }
    let Some(source) = env::var_os(PLAYBACK_DIR) else {
        if w5 {
            return Err(
                format!("W5 requires {PLAYBACK_DIR} pointing to the reviewed W4 archive").into(),
            );
        }
        return Ok(());
    };
    let source = PathBuf::from(source);
    let shape = input(&source, PLAYBACK_DIR, "shape.ghs")?;
    let tone = input(&source, PLAYBACK_DIR, "tone.ght")?;
    archive_profile::validate(&shape, &tone)?;
    if w5 {
        let label = "W5 requires the exact reviewed W4 archive";
        check_digest(&shape, W4_SHAPE, label, "shape.ghs")?;
        check_digest(&tone, W4_TONE, label, "tone.ght")?;
        println!("cargo:rustc-cfg=goodhand_w5_profile");
    }
    if w5 && desktop && native_motion.is_none() {
        return Err(format!(
            "reviewed W5 desktop playback requires {NATIVE_MOTION_DIR} pointing to its private shape.ghs and tone.ght motion archive"
        ).into());
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo did not provide OUT_DIR")?);
    if let Some(source) = native_motion {
        let shape = input(&source, NATIVE_MOTION_DIR, "shape.ghs")?;
        let tone = input(&source, NATIVE_MOTION_DIR, "tone.ght")?;
        archive_profile::validate_native_motion(&shape, &tone)?;
        let label = "W5 desktop playback requires the approved native motion archive";
        check_digest(&shape, MOTION_SHAPE, label, "shape.ghs")?;
        check_digest(&tone, MOTION_TONE, label, "tone.ght")?;
        fs::write(output.join("native-motion-shape.ghs"), shape)?;
        fs::write(output.join("native-motion-tone.ght"), tone)?;
        println!("cargo:rustc-cfg=goodhand_native_motion");
    }
    fs::write(output.join("shape.ghs"), shape)?;
    fs::write(output.join("tone.ght"), tone)?;
    println!("cargo:rustc-cfg=goodhand_embedded_playback");
    Ok(())
}
