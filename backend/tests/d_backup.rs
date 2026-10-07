//! Slice D: backup → restore into an empty dir gives identical row counts and
//! file hashes; restore refuses a non-empty data dir; hashes are verified.

mod common;

use common::d::file_hashes;
use common::spawn_app;
use pitcairn::error::AppError;

#[tokio::test]
async fn backup_then_restore_into_empty_dir_is_identical() {
    let app = spawn_app(true).await;
    let data_dir = app.state.config.data_dir.clone();
    let out = tempfile::TempDir::new().unwrap();
    let backup_dir = out.path().join("b1");

    let manifest = pitcairn::backup::backup(&app.pool, &data_dir, &backup_dir)
        .await
        .expect("backup");
    assert!(backup_dir.join("pitcairn.sqlite3").exists());
    assert!(backup_dir.join("backup.json").exists());
    assert!(!manifest.files.is_empty(), "seeded files are backed up");
    let live_counts = pitcairn::backup::row_counts(&app.pool).await.unwrap();
    assert_eq!(manifest.row_counts, live_counts);

    let target = tempfile::TempDir::new().unwrap();
    let restored = pitcairn::backup::restore(&backup_dir, target.path(), false)
        .await
        .expect("restore into empty dir");
    assert_eq!(restored, manifest);

    let pool = pitcairn::db::connect(&target.path().join("pitcairn.sqlite3"))
        .await
        .unwrap();
    assert_eq!(
        pitcairn::backup::row_counts(&pool).await.unwrap(),
        live_counts
    );
    assert_eq!(file_hashes(target.path()), file_hashes(&data_dir));
    // storage_key now points into the restored data dir.
    let keys: Vec<String> = sqlx::query_scalar("SELECT storage_key FROM files")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(!keys.is_empty());
    let prefix = target.path().to_string_lossy().to_string();
    assert!(keys.iter().all(|k| k.starts_with(&prefix)), "{keys:?}");
    pool.close().await;

    // A second backup into the same directory is refused.
    assert!(matches!(
        pitcairn::backup::backup(&app.pool, &data_dir, &backup_dir).await,
        Err(AppError::Conflict { .. })
    ));
}

#[tokio::test]
async fn restore_refuses_non_empty_dir_and_tampered_backups() {
    let app = spawn_app(true).await;
    let data_dir = app.state.config.data_dir.clone();
    let out = tempfile::TempDir::new().unwrap();
    let backup_dir = out.path().join("b");
    pitcairn::backup::backup(&app.pool, &data_dir, &backup_dir)
        .await
        .unwrap();

    // The live data dir is not empty → refused without --force.
    match pitcairn::backup::restore(&backup_dir, &data_dir, false).await {
        Err(AppError::Conflict { code, .. }) => assert_eq!(code, "data_dir_not_empty"),
        other => panic!("expected data_dir_not_empty, got {other:?}"),
    }

    // Tamper with one stored file → hash verification fails, nothing restored.
    let manifest: pitcairn::backup::BackupManifest =
        serde_json::from_slice(&std::fs::read(backup_dir.join("backup.json")).unwrap()).unwrap();
    let (rel, _) = manifest.files.iter().next().unwrap();
    std::fs::write(backup_dir.join("files").join(rel), b"tampered").unwrap();
    let target = tempfile::TempDir::new().unwrap();
    match pitcairn::backup::restore(&backup_dir, target.path(), false).await {
        Err(AppError::Unprocessable { code, .. }) => assert_eq!(code, "checksum_mismatch"),
        other => panic!("expected checksum_mismatch, got {other:?}"),
    }
    assert!(!target.path().join("pitcairn.sqlite3").exists());
}

#[tokio::test]
async fn restore_with_force_replaces_existing_data() {
    let app = spawn_app(true).await;
    let data_dir = app.state.config.data_dir.clone();
    let out = tempfile::TempDir::new().unwrap();
    let backup_dir = out.path().join("b");
    pitcairn::backup::backup(&app.pool, &data_dir, &backup_dir)
        .await
        .unwrap();

    // Another non-empty install gets overwritten only with force.
    let other = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(other.path().join("files/ab")).unwrap();
    std::fs::write(other.path().join("files/ab/stray"), b"old").unwrap();
    std::fs::write(other.path().join("pitcairn.sqlite3"), b"").unwrap();
    assert!(
        pitcairn::backup::restore(&backup_dir, other.path(), false)
            .await
            .is_err()
    );
    pitcairn::backup::restore(&backup_dir, other.path(), true)
        .await
        .expect("forced restore");
    assert!(!other.path().join("files/ab/stray").exists());
    assert_eq!(file_hashes(other.path()), file_hashes(&data_dir));
}
