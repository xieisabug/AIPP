use sha2::{Digest, Sha256};
use std::path::Path;

pub mod process;
pub mod js_bridge;
pub mod wasm;

pub(crate) fn verify_entry_checksum(
    path: &Path,
    expected_checksum: Option<&str>,
) -> Result<(), String> {
    let Some(expected_checksum) =
        expected_checksum.map(str::trim).filter(|checksum| !checksum.is_empty())
    else {
        return Ok(());
    };
    let expected = expected_checksum
        .strip_prefix("sha256:")
        .unwrap_or(expected_checksum)
        .trim()
        .to_ascii_lowercase();
    let bytes = std::fs::read(path)
        .map_err(|error| format!("Failed to read plugin entry '{}': {}", path.display(), error))?;
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "Plugin entry checksum mismatch for '{}': expected {}, got {}",
            path.display(),
            expected,
            actual
        ))
    }
}
