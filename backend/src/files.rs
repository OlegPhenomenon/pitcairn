//! File storage: content-addressed bytes under `data/files/<aa>/<sha256>`,
//! temp upload parts under `data/uploads/<upload_id>.part`.

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};

pub const CHUNK_SIZE: u64 = 5 * 1024 * 1024; // 5 MiB

/// Read buffer for hashing and scanning stored files.
const READ_BUF: usize = 1024 * 1024;

pub fn part_path(data_dir: &Path, upload_id: &str) -> PathBuf {
    data_dir.join("uploads").join(format!("{upload_id}.part"))
}

/// Exactly 64 lowercase hex characters — the only shape a stored file name
/// may have.
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn checked_sha256(sha256: &str) -> AppResult<&str> {
    if is_sha256_hex(sha256) {
        Ok(sha256)
    } else {
        Err(AppError::internal(format!(
            "refusing storage path for invalid sha256 {sha256:?}"
        )))
    }
}

/// `files/<aa>/<sha256>` relative to the data dir (export archives use it).
pub fn storage_rel(sha256: &str) -> AppResult<String> {
    let sha256 = checked_sha256(sha256)?;
    Ok(format!("files/{}/{}", &sha256[0..2], sha256))
}

/// Storage path of a content-addressed file. Fails (never panics) unless
/// `sha256` is 64 lowercase hex, so the result always stays inside
/// `data_dir/files`.
pub fn file_path(data_dir: &Path, sha256: &str) -> AppResult<PathBuf> {
    let sha256 = checked_sha256(sha256)?;
    Ok(data_dir.join("files").join(&sha256[0..2]).join(sha256))
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
        .truncate(false)
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
    let mut hasher = sha2::Sha256::new();
    for_each_chunk(path, |chunk| hasher.update(chunk)).await?;
    Ok(hex::encode(hasher.finalize()))
}

/// Feed a file to `f` in bounded chunks (never the whole file in memory).
pub async fn for_each_chunk(path: &Path, mut f: impl FnMut(&[u8])) -> AppResult<()> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; READ_BUF];
    loop {
        let read = file.read(&mut buf).await?;
        if read == 0 {
            break;
        }
        f(&buf[..read]);
    }
    Ok(())
}

/// Move the part file into content-addressed storage atomically
/// (write to temp name in the target dir, then rename).
pub async fn store_file(data_dir: &Path, part: &Path, sha256: &str) -> AppResult<PathBuf> {
    let target = file_path(data_dir, sha256)?;
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
    let path = file_path(data_dir, sha256)?;
    Ok(tokio::fs::read(path).await?)
}

/// A stored file as a streaming response body plus its on-disk length;
/// bytes are read from disk as the client consumes them.
pub async fn stream_file(data_dir: &Path, sha256: &str) -> AppResult<(axum::body::Body, u64)> {
    let path = file_path(data_dir, sha256)?;
    let file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    let body =
        axum::body::Body::from_stream(tokio_util::io::ReaderStream::with_capacity(file, 64 * 1024));
    Ok((body, len))
}
