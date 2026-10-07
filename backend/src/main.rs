use std::sync::Arc;

use clap::{Parser, Subcommand};
use pitcairn::config::Config;
use pitcairn::error::AppResult;
use pitcairn::{AppState, db, jobs, seed};

#[derive(Parser)]
#[command(name = "pitcairn", about = "Pitcairn Research Permits & Data Hub")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server (applies migrations, starts the job worker).
    Serve,
    /// Apply database migrations and exit.
    Migrate,
    /// Seed demo personas, settings and templates. `--reset` wipes the
    /// database file first.
    SeedDemo {
        #[arg(long)]
        reset: bool,
    },
    /// Create a user with the admin role and print a generated password.
    CreateAdmin {
        #[arg(long)]
        email: String,
        #[arg(long)]
        name: String,
    },
    /// Grant a role to an existing user (server operator only; the only way
    /// to bootstrap the first decision_maker/admin).
    GrantRole {
        #[arg(long)]
        email: String,
        #[arg(long)]
        role: String,
    },
    /// Export TypeScript bindings for all API DTOs.
    ExportTypes,
}

#[tokio::main]
async fn main() -> AppResult<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pitcairn=info,tower_http=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let config = Config::from_env();

    match cli.command {
        Command::Serve => serve(config).await,
        Command::Migrate => {
            config.prepare()?;
            let pool = db::connect(&config.db_path()).await?;
            db::migrate(&pool).await?;
            println!("migrations applied");
            Ok(())
        }
        Command::SeedDemo { reset } => {
            config.prepare()?;
            if reset {
                let db = config.db_path();
                let wal = std::path::PathBuf::from(format!("{}-wal", db.display()));
                let shm = std::path::PathBuf::from(format!("{}-shm", db.display()));
                for path in [db, wal, shm] {
                    tokio::fs::remove_file(&path).await.ok();
                }
            }
            let pool = db::connect(&config.db_path()).await?;
            db::migrate(&pool).await?;
            seed::seed_demo(&pool).await?;
            println!("demo data seeded");
            Ok(())
        }
        Command::CreateAdmin { email, name } => {
            config.prepare()?;
            let pool = db::connect(&config.db_path()).await?;
            db::migrate(&pool).await?;
            let password = seed::create_admin(&pool, &email, &name).await?;
            println!("created admin user {email}");
            println!("password: {password}");
            Ok(())
        }
        Command::GrantRole { email, role } => {
            config.prepare()?;
            let pool = db::connect(&config.db_path()).await?;
            db::migrate(&pool).await?;
            seed::grant_role(&pool, &email, &role).await?;
            println!("granted {role} to {email}");
            Ok(())
        }
        Command::ExportTypes => {
            pitcairn::dto::export_all().map_err(|e| pitcairn::error::AppError::internal(e))?;
            println!("exported TypeScript bindings to {}", pitcairn::dto::EXPORT_DIR);
            Ok(())
        }
    }
}

async fn serve(config: Config) -> AppResult<()> {
    config.prepare()?;
    let pool = db::connect(&config.db_path()).await?;
    db::migrate(&pool).await?;

    if !config.demo_mode {
        // Production installs never keep persona-switch sessions (§3).
        sqlx::query("DELETE FROM sessions WHERE via_demo_switch = 1")
            .execute(&pool)
            .await?;
    }

    let config = Arc::new(config);
    let mail = Arc::new(pitcairn::mail::DemoMailbox::new(pool.clone()));
    let state = AppState::new(pool.clone(), config.clone(), mail);

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let worker = tokio::spawn(jobs::worker_loop(state.clone(), shutdown_rx));

    let app = pitcairn::build_app(state);
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!(bind = %config.bind, demo = config.demo_mode, "pitcairn listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Graceful stop: stop taking jobs, let the current one finish, close pool.
    let _ = shutdown_tx.send(true);
    let _ = worker.await;
    pool.close().await;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("install Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
