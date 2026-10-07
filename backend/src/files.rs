//! File storage: content-addressed bytes under `data/files/<aa>/<sha256>`,
//! temp upload parts under `data/uploads/<upload_id>.part`.

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};

pub const CHUNK_SIZE: u64 = 5 * 1024 * 1024; // 5 MiB

pub fn part_path(data_dir: &Path, upload_id: &str) -> PathBuf {
    data_dir.join("uploads").join(format!("{upload_id}.part"))
}

pub fn file_path(data_dir: &Path, sha256: &str) -> PathBuf {
    data_dir.join("files").join(&sha256[0..2]).join(sha256)
}

/// Write `bytes` for chunk `n` into the upload's part file at the right offset.
pub async fn write_chunk(
    data_dir: &Path,
    upload_id: &str,
    n: u64,
    chunk_size: u64,
    bytes: &[u8],
) -> AppResult<()> {
    let path = part_path(data_dir, upload_id);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&path)
        .await?;
    use tokio::io::AsyncSeekExt;
    file.seek(std::io::SeekFrom::Start(n * chunk_size)).await?;
    file.write_all(bytes).await?;
    file.flush().await?;
    Ok(())
}

/// sha256 of a file, streamed. Called OUTSIDE any DB transaction.
pub async fn hash_file(path: &Path) -> AppResult<String> {
    use sha2::Digest;
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Move the part file into content-addressed storage atomically
/// (write to temp name in the target dir, then rename).
pub async fn store_file(data_dir: &Path, part: &Path, sha256: &str) -> AppResult<PathBuf> {
    let target = file_path(data_dir, sha256);
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if target.exists() {
        // Dedup: bytes already stored (identical sha256).
        tokio::fs::remove_file(part).await.ok();
        return Ok(target);
    }
    let tmp = target.with_extension("tmp");
    tokio::fs::rename(part, &tmp).await.map_err(|e| {
        AppError::internal(std::io::Error::new(
            e.kind(),
            format!("move into storage failed: {e}"),
        ))
    })?;
    tokio::fs::rename(&tmp, &target).await?;
    Ok(target)
}

pub async fn read_file(data_dir: &Path, sha256: &str) -> AppResult<Vec<u8>> {
    let path = file_path(data_dir, sha256);
    Ok(tokio::fs::read(path).await?)
}
