//! File helpers shared by the subcommands.

use std::path::Path;

use anyhow::{Context, Result};

pub fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

/// Writes `contents` to `path` atomically: a partially written scene file is
/// worse than no output at all, so the bytes land in a sibling temp file and are
/// renamed into place only once they are fully on disk.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write as _;

    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(
        ".{}.gdmerge.{}.{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("out"),
        std::process::id(),
        stamp
    ));

    let result = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("writing {}", tmp.display()));
    }
    std::fs::rename(&tmp, path)
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
        .with_context(|| format!("replacing {}", path.display()))
}
