use clap::{Parser, Subcommand};
use std::ffi::OsString;
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
    },
    /// Import a provider export file.
    Import { provider: String, path: PathBuf },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Migrate { db } => {
            let path = shellexpand(&db);
            let storage_config = kfx_core::StorageConfig {
                path: path.to_string_lossy().to_string(),
                use_encryption: false,
            };
            let pool = kfx_core::storage::open(&path, &storage_config).await?;
            kfx_core::storage::migrate(&pool).await?;
            println!("migrations applied to {}", path.display());
        }
        Cmd::Import { provider, path } => {
            println!(
                "import {} from {}: adapter not yet wired",
                provider,
                path.display()
            );
            // TODO(#3): wire adapter
        }
    }
    Ok(())
}

/// Expand a leading `~/` against the user's home directory.
fn shellexpand(p: &str) -> PathBuf {
    shellexpand_with(p, home_dir())
}

/// The user's home directory, in the order the platforms actually set it.
///
/// `HOME` is the POSIX convention and is what Linux and macOS set. Windows
/// normally sets `USERPROFILE` and frequently leaves `HOME` unset, so a
/// `HOME`-only lookup silently fails to expand `~/...` there and creates a
/// directory literally named `~`. Empty values are treated as unset so an
/// empty `HOME` cannot shadow a usable `USERPROFILE`.
fn home_dir() -> Option<OsString> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
}

/// Pure form of [`shellexpand`], taking the home directory as an argument.
///
/// Keeping the environment lookup out of this function makes the behaviour
/// testable without mutating process-global environment variables, which
/// would race against other tests running in parallel.
fn shellexpand_with(p: &str, home: Option<OsString>) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = home {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_tilde_against_home() {
        let out = shellexpand_with(
            "~/.kinexfluence/kinexfluence.db",
            Some(OsString::from("/home/u")),
        );
        assert_eq!(out, PathBuf::from("/home/u/.kinexfluence/kinexfluence.db"));
    }

    #[test]
    fn leaves_tilde_when_no_home_is_known() {
        let out = shellexpand_with("~/.kinexfluence/kinexfluence.db", None);
        assert_eq!(out, PathBuf::from("~/.kinexfluence/kinexfluence.db"));
    }

    #[test]
    fn leaves_absolute_paths_untouched() {
        let out = shellexpand_with("/var/data/k.db", Some(OsString::from("/home/u")));
        assert_eq!(out, PathBuf::from("/var/data/k.db"));
    }

    #[test]
    fn home_dir_prefers_home_and_falls_back_to_userprofile() {
        let saved_home = std::env::var_os("HOME");
        let saved_profile = std::env::var_os("USERPROFILE");

        // Both set: HOME wins.
        std::env::set_var("HOME", "/home/posix");
        std::env::set_var("USERPROFILE", r"C:\Users\win");
        assert_eq!(home_dir(), Some(OsString::from("/home/posix")));

        // The Windows case: HOME unset, USERPROFILE must be used.
        std::env::remove_var("HOME");
        assert_eq!(home_dir(), Some(OsString::from(r"C:\Users\win")));

        // An empty HOME must not shadow a usable USERPROFILE.
        std::env::set_var("HOME", "");
        assert_eq!(home_dir(), Some(OsString::from(r"C:\Users\win")));

        // Neither set: no expansion happens.
        std::env::remove_var("HOME");
        std::env::remove_var("USERPROFILE");
        assert_eq!(home_dir(), None);

        match saved_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match saved_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}
