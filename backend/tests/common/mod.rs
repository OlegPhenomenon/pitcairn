#![allow(dead_code)]

pub mod b;

use std::sync::Arc;

use reqwest::header::{HeaderMap, HeaderValue};

pub struct TestApp {
    pub base_url: String,
    pub state: pitcairn::AppState,
    pub pool: sqlx::SqlitePool,
    pub _dir: tempfile::TempDir,
}

pub async fn spawn_app(demo_mode: bool) -> TestApp {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let data_dir = dir.path().to_path_buf();

    let config = pitcairn::config::Config {
        bind: "127.0.0.1:0".into(),
        data_dir: data_dir.clone(),
        static_dir: data_dir.join("nonexistent"),
        base_url: "http://localhost".into(),
        demo_mode,
        session_secret: "test-session-secret-0000000000000000000000000000".into(),
        session_secret_generated: false,
        bank_webhook_secret: "test-bank-webhook-secret-000000000000000000000000".into(),
        bank_webhook_secret_generated: false,
        max_upload_bytes: 2_147_483_648,
        link_check_mode: "mock".into(),
        ai_mode: "mock".into(),
        secure_cookies: false,
    };

    config.prepare().expect("prepare config");
    let pool = pitcairn::db::connect(&config.db_path())
        .await
        .expect("connect db");
    pitcairn::db::migrate(&pool).await.expect("migrate db");
    pitcairn::seed::seed_demo(&pool).await.expect("seed demo");

    let config = Arc::new(config);
    let mail = Arc::new(pitcairn::mail::DemoMailbox::new(pool.clone()));
    let state = pitcairn::AppState::new(pool.clone(), config.clone(), mail);

    let app = pitcairn::build_app(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .expect("serve");
    });

    TestApp {
        base_url: format!("http://127.0.0.1:{port}"),
        state,
        pool,
        _dir: dir,
    }
}

pub struct Client {
    pub http: reqwest::Client,
    pub base: String,
}

impl Client {
    pub fn anonymous(app: &TestApp) -> Client {
        let http = reqwest::Client::builder()
            .cookie_store(true)
            .build()
            .expect("build http client");
        Client {
            http,
            base: app.base_url.clone(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut headers = HeaderMap::new();
        headers.insert("X-Pitcairn-Csrf", HeaderValue::from_static("1"));
        self.http.request(method, self.url(path)).headers(headers)
    }

    pub fn request_no_csrf(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, self.url(path))
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.request(reqwest::Method::GET, path)
            .send()
            .await
            .expect("send get")
    }

    pub async fn post(&self, path: &str) -> reqwest::Response {
        self.request(reqwest::Method::POST, path)
            .send()
            .await
            .expect("send post")
    }

    pub async fn post_json<B: serde::Serialize>(&self, path: &str, body: &B) -> reqwest::Response {
        self.request(reqwest::Method::POST, path)
            .json(body)
            .send()
            .await
            .expect("send post json")
    }

    pub async fn put_json<B: serde::Serialize>(&self, path: &str, body: &B) -> reqwest::Response {
        self.request(reqwest::Method::PUT, path)
            .json(body)
            .send()
            .await
            .expect("send put json")
    }

    pub async fn patch_json<B: serde::Serialize>(&self, path: &str, body: &B) -> reqwest::Response {
        self.request(reqwest::Method::PATCH, path)
            .json(body)
            .send()
            .await
            .expect("send patch json")
    }

    pub async fn delete(&self, path: &str) -> reqwest::Response {
        self.request(reqwest::Method::DELETE, path)
            .send()
            .await
            .expect("send delete")
    }

    pub async fn json<T: serde::de::DeserializeOwned>(&self, resp: reqwest::Response) -> T {
        let status = resp.status();
        let text = resp.text().await.expect("read body");
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("json parse failed (status {status}): {e}\nbody: {text}"))
    }
}

pub async fn login(app: &TestApp, email: &str, password: &str) -> Client {
    let client = Client::anonymous(app);
    let resp = client
        .post_json(
            "/api/v1/auth/login",
            &serde_json::json!({"email": email, "password": password}),
        )
        .await;
    assert_eq!(resp.status(), 200, "login should succeed");
    client
}

pub async fn persona(app: &TestApp, key: &str) -> Client {
    let persona = pitcairn::seed::PERSONAS
        .iter()
        .find(|p| p.key == key)
        .unwrap_or_else(|| panic!("unknown persona {key}"));

    let client = Client::anonymous(app);
    let resp = client
        .post_json(
            "/api/v1/auth/login",
            &serde_json::json!({"email": persona.email, "password": pitcairn::seed::DEMO_PASSWORD}),
        )
        .await;
    assert_eq!(resp.status(), 200, "persona login should succeed");
    let login: pitcairn::dto::LoginResponse = client.json(resp).await;

    if login.mfa_required {
        let user_id = &login.user.id;
        let totp_resp = client.get(&format!("/api/v1/demo/totp/{user_id}")).await;
        assert_eq!(totp_resp.status(), 200, "demo totp should be available");
        let totp: pitcairn::dto::DemoTotpResponse = client.json(totp_resp).await;

        let verify_resp = client
            .post_json(
                "/api/v1/auth/mfa/verify",
                &serde_json::json!({"code": totp.code}),
            )
            .await;
        assert_eq!(verify_resp.status(), 200, "mfa verify should succeed");
    }

    client
}
