// Intent citation: .kiro/specs/model-download-engine/design.md — Disk Space Management
// Disk space checking and monitoring for download safety.

use std::path::Path;

/// Error returned when disk space is insufficient.
#[derive(Debug, Clone)]
pub struct DiskSpaceError {
    pub available_mb: u64,
    pub required_mb: u64,
}

impl std::fmt::Display for DiskSpaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Insufficient disk space: need {}MB, have {}MB available",
            self.required_mb, self.available_mb
        )
    }
}

/// Check if there is enough disk space to start a download.
///
/// # Arguments
/// * `dir` - The directory where the file will be stored
/// * `file_size_bytes` - Size of the file to download
/// * `min_buffer_mb` - Minimum buffer space to keep free (default: 1024MB = 1GB)
///
/// # Returns
/// Ok(()) if space is sufficient, Err(DiskSpaceError) otherwise.
pub fn check_disk_space(
    dir: &Path,
    file_size_bytes: u64,
    min_buffer_mb: u64,
) -> Result<(), DiskSpaceError> {
    let available_bytes = get_available_space(dir);
    let file_size_mb = file_size_bytes / (1024 * 1024);
    let required_mb = file_size_mb + min_buffer_mb;
    let available_mb = available_bytes / (1024 * 1024);

    if available_mb >= required_mb {
        Ok(())
    } else {
        Err(DiskSpaceError {
            available_mb,
            required_mb,
        })
    }
}

/// Check if disk space is critically low (below threshold).
/// Used for periodic monitoring during downloads.
///
/// # Arguments
/// * `dir` - The directory to check
/// * `threshold_mb` - Minimum acceptable free space in MB (default: 500)
///
/// # Returns
/// true if space is below threshold (critically low).
pub fn is_space_critically_low(dir: &Path, threshold_mb: u64) -> bool {
    let available_bytes = get_available_space(dir);
    let available_mb = available_bytes / (1024 * 1024);
    available_mb < threshold_mb
}

/// Get available disk space in bytes for the given path.
/// Uses the `sysinfo` crate for cross-platform disk info.
fn get_available_space(dir: &Path) -> u64 {
    use sysinfo::Disks;

    let disks = Disks::new_with_refreshed_list();

    // Find the disk that contains our directory
    let dir_canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());

    let mut best_match: Option<(usize, u64)> = None;

    for disk in disks.list() {
        let mount = disk.mount_point();
        if dir_canonical.starts_with(mount) {
            let mount_len = mount.as_os_str().len();
            match &best_match {
                Some((len, _)) if mount_len > *len => {
                    best_match = Some((mount_len, disk.available_space()));
                }
                None => {
                    best_match = Some((mount_len, disk.available_space()));
                }
                _ => {}
            }
        }
    }

    best_match.map(|(_, space)| space).unwrap_or(0)
}

/// Get available disk space in MB (public helper for status reporting).
pub fn available_space_mb(dir: &Path) -> u64 {
    get_available_space(dir) / (1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_check_disk_space_sufficient() {
        // Use current directory — should have some space available
        let dir = PathBuf::from(".");
        // Request 1 byte with 0 buffer — should always pass
        let result = check_disk_space(&dir, 1, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_disk_space_insufficient() {
        let dir = PathBuf::from(".");
        // Request an absurdly large amount
        let result = check_disk_space(&dir, u64::MAX / 2, u64::MAX / 4);
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.required_mb > e.available_mb);
        }
    }

    #[test]
    fn test_is_space_critically_low_with_high_threshold() {
        let dir = PathBuf::from(".");
        // With a threshold of u64::MAX MB, space should always be "critically low"
        assert!(is_space_critically_low(&dir, u64::MAX));
    }

    #[test]
    fn test_is_space_critically_low_with_zero_threshold() {
        let dir = PathBuf::from(".");
        // With threshold of 0, space should never be critically low
        assert!(!is_space_critically_low(&dir, 0));
    }

    #[test]
    fn test_available_space_mb_returns_nonzero() {
        let dir = PathBuf::from(".");
        let space = available_space_mb(&dir);
        // On any real system, current directory should have some space
        assert!(space > 0, "Expected non-zero available space");
    }

    #[test]
    fn test_disk_space_error_display() {
        let err = DiskSpaceError {
            available_mb: 500,
            required_mb: 2000,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("500"));
        assert!(msg.contains("2000"));
    }
}
