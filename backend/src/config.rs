use std::path::PathBuf;

use rand::RngCore;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: String,
    pub data_dir: PathBuf,
    pub static_dir: PathBuf,
    pub base_url: String,
    pub demo_mode: bool,
    pub session_secret: String,
    pub session_secret_generated: bool,
    pub bank_webhook_secret: String,
    pub bank_webhook_secret_generated: bool,
    pub max_upload_bytes: u64,
    pub link_check_mode: String,
    pub ai_mode: String,
    pub secure_cookies: bool,
}

fn random_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

impl Config {
    pub fn from_env() -> Self {
        let env = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
        let (session_secret, session_secret_generated) = match env("PITCAIRN_SESSION_SECRET") {
            Some(s) => (s, false),
            None => (random_secret(), true),
        };
        let (bank_webhook_secret, bank_webhook_secret_generated) =
            match env("PITCAIRN_BANK_WEBHOOK_SECRET") {
                Some(s) => (s, false),
                None => (random_secret(), true),
            };
        Config {
            bind: env("PITCAIRN_BIND").unwrap_or_else(|| "127.0.0.1:8080".into()),
            data_dir: env("PITCAIRN_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("./data")),
            static_dir: env("PITCAIRN_STATIC_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("../frontend/dist")),
            base_url: env("PITCAIRN_BASE_URL").unwrap_or_else(|| "http://localhost:8080".into()),
            demo_mode: env("PITCAIRN_DEMO_MODE").as_deref() == Some("true"),
            session_secret,
            session_secret_generated,
            bank_webhook_secret,
            bank_webhook_secret_generated,
            max_upload_bytes: env("PITCAIRN_MAX_UPLOAD_BYTES")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2_147_483_648),
            link_check_mode: env("PITCAIRN_LINK_CHECK_MODE").unwrap_or_else(|| "mock".into()),
            ai_mode: env("PITCAIRN_AI_MODE").unwrap_or_else(|| "mock".into()),
            secure_cookies: env("PITCAIRN_SECURE_COOKIES").as_deref() == Some("true"),
        }
    }

    /// Create data directories (idempotent) and warn about generated secrets.
    pub fn prepare(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.data_dir.join("files"))?;
        std::fs::create_dir_all(self.data_dir.join("uploads"))?;
        std::fs::create_dir_all(self.data_dir.join("backups"))?;
        if self.session_secret_generated {
            tracing::warn!(
                "PITCAIRN_SESSION_SECRET not set; generated a random secret for this start \
                 (all sessions are invalidated on restart)"
            );
        }
        if self.bank_webhook_secret_generated {
            tracing::warn!(
                "PITCAIRN_BANK_WEBHOOK_SECRET not set; generated a random secret for this start"
            );
        }
        Ok(())
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_dir.join("pitcairn.sqlite3")
    }
}
