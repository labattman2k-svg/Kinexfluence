use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "kinexfluence", version, about = "Kinexfluence CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Apply database migrations.
    Migrate {
        #[arg(long, default_value = "~/.kinexfluence/kinexfluence.db")]
        db: String,
        #[arg(long, default_value = "true")]
        encrypt: bool,
    },
    /// Import a provider export file.
    Import {
        provider: String,
        path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Migrate { db, encrypt } => {
            let path = shellexpand(&db);
            let storage_config = kfx_core::StorageConfig {
                path: db,
                use_encryption: encrypt,
            };
            let pool = kfx_core::storage::open(&path, &storage_config).await?;
            kfx_core::storage::migrate(&pool).await?;
            println!("migrations applied to {}", path.display());
        }
        Cmd::Import { provider, path } => {
            println!("import {} from {}", provider, path.display());
            // TODO: wire adapter
        }
    }
    Ok(())
}

fn shellexpand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}
