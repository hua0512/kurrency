//! The on-disk rate cache: JSON under `{cache_dir}/kurrency/rates.json`,
//! written atomically (temporary file in the same directory, then rename).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::provider::RateTable;

/// The default cache file, or `None` when the platform has no cache dir.
pub fn default_path() -> Option<PathBuf> {
    crate::paths::cache_dir().map(|dir| dir.join("rates.json"))
}

/// Reads a cached table; a missing or unreadable cache is `None`.
pub fn load(path: &Path) -> Option<RateTable> {
    let bytes = fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(table) => Some(table),
        Err(err) => {
            log::warn!("ignoring unreadable rate cache {}: {err}", path.display());
            None
        }
    }
}

/// Writes `table` to `path` atomically.
pub fn save(path: &Path, table: &RateTable) -> anyhow::Result<()> {
    let json = serde_json::to_vec_pretty(table)?;
    write_atomic(path, &json)
}

/// Writes `bytes` to a sibling temporary file, syncs it, then renames it over
/// `path`, so a crash never leaves a half-written file behind.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "data".into());
    let tmp = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    Ok(result?)
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;
    use crate::rates::provider::Source;

    #[test]
    fn round_trips_atomically() {
        let dir = std::env::temp_dir().join(format!("kurrency-cache-test-{}", std::process::id()));
        let path = dir.join("nested").join("rates.json");
        let table = RateTable {
            base: "USD".into(),
            rates: [("EUR".to_string(), Decimal::new(8956, 4))].into(),
            dates: [("EUR".to_string(), "2026-11-06".to_string())].into(),
            source: Source::Fawaz,
            fetched_at: 123,
        };
        assert_eq!(load(&path), None);
        save(&path, &table).unwrap();
        assert_eq!(load(&path), Some(table.clone()));
        // Overwrite in place, and leave no temporary files behind.
        save(&path, &table).unwrap();
        let entries: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(entries.len(), 1);
        fs::write(&path, b"garbage").unwrap();
        assert_eq!(load(&path), None);
        let _ = fs::remove_dir_all(dir);
    }
}
