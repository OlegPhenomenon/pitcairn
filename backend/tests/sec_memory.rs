//! Security regressions for memory exhaustion: import endpoints reject
//! non-admins before reading the request body, file downloads stream from
//! disk, and the scanner works on bounded chunks.

mod common;

use std::time::Duration;

use common::c::{create_document, seeded_project, sha256, upload_clean_file, user_id};
use common::{persona, spawn_app};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Send headers announcing a 1 GiB body plus a few bytes of it, then wait
/// for the response without ever sending the rest.
async fn post_huge_body(base_url: &str, path: &str, token: &str) -> Option<String> {
    let addr = base_url.trim_start_matches("http://");
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nCookie: {}={token}\r\n\
         X-Pitcairn-Csrf: 1\r\nContent-Type: application/zip\r\n\
         Content-Length: 1073741824\r\n\r\n",
        pitcairn::authz::SESSION_COOKIE
    );
    stream.write_all(head.as_bytes()).await.unwrap();
    stream.write_all(&[0u8; 4096]).await.unwrap();
    let mut buf = vec![0u8; 1024];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await;
    match read {
        Ok(Ok(n)) if n > 0 => Some(String::from_utf8_lossy(&buf[..n]).to_string()),
        _ => None,
    }
}

#[tokio::test]
async fn import_endpoints_reject_non_admins_before_reading_the_body() {
    let app = spawn_app(true).await;
    // A plain registered user's session.
    let lukas = user_id(&app, "lukas@demo.pitcairn.invalid").await;
    let token = "sec-memory-test-session-token";
    sqlx::query(
        "INSERT INTO sessions (id, user_id, created_at, expires_at, mfa_verified, via_demo_switch)
         VALUES (?, ?, '2026-01-01T00:00:00Z', '2999-01-01T00:00:00Z', 0, 0)",
    )
    .bind(pitcairn::util::sha256_hex(token.as_bytes()))
    .bind(&lukas)
    .execute(&app.pool)
    .await
    .unwrap();

    for path in [
        "/api/v1/admin/import/project-archive",
        "/api/v1/admin/import/legacy/preview",
    ] {
        let resp = post_huge_body(&app.base_url, path, token)
            .await
            .unwrap_or_else(|| panic!("{path}: no response before the body was sent"));
        assert!(resp.starts_with("HTTP/1.1 403"), "{path}: {resp}");
    }
}

#[tokio::test]
async fn scanner_detects_signatures_across_chunk_boundaries() {
    use pitcairn::jobs::{Scanner, scan_bytes};
    let eicar: &[u8] = b"X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";
    let mut data = vec![b'a'; 3000];
    data.splice(1000..1000, eicar.iter().copied());
    assert!(scan_bytes(&data, "text/plain").is_some());
    // Every chunking (including 1-byte chunks) gives the same verdict.
    for size in [1, 7, 67, 68, 69, 1000, 1001, 4096] {
        let mut s = Scanner::new();
        for chunk in data.chunks(size) {
            s.update(chunk);
        }
        assert!(s.finish("text/plain").is_some(), "chunk size {size}");
    }
    // Magic numbers split over chunks.
    let mut s = Scanner::new();
    for b in b"MZ rest of an exe" {
        s.update(std::slice::from_ref(b));
    }
    assert!(s.finish("text/plain").is_some());
    // Clean text stays clean; binary after a text prefix is no longer text.
    let mut s = Scanner::new();
    s.update(b"site,date,variable,value,unit\n");
    s.update(b"A,2024-01-01,temp,1.5,C\n");
    assert_eq!(s.finish("text/csv"), None);
    let mut s = Scanner::new();
    s.update(b"plain text");
    s.update(&[0xff, 0x00, 0x01]);
    assert_eq!(s.finish("application/octet-stream"), None);
}

#[tokio::test]
async fn large_downloads_stream_the_stored_bytes() {
    let app = spawn_app(true).await;
    let anna = persona(&app, "anna").await;
    let pid = seeded_project(&app).await;
    // 4 MiB of text: several scanner read buffers and response chunks.
    let bytes: Vec<u8> = (0..4 * 1024 * 1024)
        .map(|i| b"abcdefghij\n"[i % 11])
        .collect();
    let file_id = upload_clean_file(&anna, &app, &bytes, "text/plain").await;
    let version = create_document(&anna, &pid, "Big notes", "other", &file_id).await;
    let resp = anna
        .get(&format!("/api/v1/document-versions/{version}/download"))
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()["content-length"].to_str().unwrap(),
        bytes.len().to_string()
    );
    let body = resp.bytes().await.unwrap();
    assert_eq!(sha256(&body), sha256(&bytes));
}
