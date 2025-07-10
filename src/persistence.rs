use crate::error::LazyError;

use bincode::{decode_from_slice, encode_to_vec};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn load_paths<P: AsRef<Path>>(path: P) -> Result<Vec<PathBuf>, LazyError> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut buffer = Vec::new();

    file.read_to_end(&mut buffer).await?;

    let result = tokio::task::spawn_blocking(move || {
        let cfg = bincode::config::standard();
        decode_from_slice::<Vec<PathBuf>, _>(&buffer, cfg).map(|(v, _)| v)
    })
    .await??;

    Ok(result)
}

pub async fn save_paths<P: AsRef<Path>>(paths: &[PathBuf], path: P) -> Result<(), LazyError> {
    let paths_owned = paths.to_vec();

    let encoded = tokio::task::spawn_blocking(move || {
        let cfg = bincode::config::standard();
        encode_to_vec(&paths_owned, cfg)
    })
    .await??;

    let mut file = tokio::fs::File::create(path).await?;
    file.write_all(&encoded).await?;

    Ok(())
}
