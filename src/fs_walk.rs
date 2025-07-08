use rayon::iter::{IntoParallelIterator, ParallelBridge, ParallelIterator};
use std::collections::HashSet;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};

/// Recursively traverse the directory tree from `dir` in parallel,
/// returning a list of file paths (excluding directories).
pub fn walk_dir_par(dir: &Path) -> Vec<PathBuf> {
    match fs::read_dir(dir) {
        Ok(read_dir) => read_dir
            .par_bridge()
            .flat_map(|entry_result| match entry_result {
                Ok(entry) => collect_entry_paths(entry),
                Err(_) => vec![],
            })
            .collect(),
        Err(_) => vec![],
    }
}

/// Handle one directory entry: recurse if it's a dir, return path if it's a file.
fn collect_entry_paths(entry: DirEntry) -> Vec<PathBuf> {
    let path = entry.path();

    match entry.file_type() {
        Ok(file_type) if file_type.is_file() => vec![path],
        Ok(file_type) if file_type.is_dir() && !should_ignore_walk(&entry) => walk_dir_par(&path),
        _ => vec![],
    }
}

// FIXME currently this would only work at the root directory.
// FIXME Rather make it apply to the last path component?
/// Logic used to decide whether a specific directory should
/// be traversed during fs walk.
fn should_ignore_walk(entry: &DirEntry) -> bool {
    const EXCLUDED_DIRS: &[&str] = &["/$Recycle.Bin/", "/$SysReset", "/Windows"];
    let path = entry.path();

    EXCLUDED_DIRS.iter().any(|excluded| {
        let excluded_path = Path::new(excluded);
        path.starts_with(excluded_path)
    })
}

pub async fn unique_parent_dirs(paths: &[PathBuf]) -> Vec<String> {
    let mut unique_dirs = HashSet::new();

    for path in paths {
        if let Some(parent) = path.parent() {
            unique_dirs.insert(parent.to_path_buf());
        }
    }
    unique_dirs
        .into_par_iter()
        .map(|pb| pb.to_string_lossy().to_string())
        .collect()
}
