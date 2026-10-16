use clap::{Parser, Subcommand};
use kfx_adapters::chatgpt::ChatGptAdapter;
use kfx_adapters::ProviderAdapter;
use kfx_core::writer::ImportSummary;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Exit codes, distinct so a script can tell a bad export from a bad database.
///
/// 2, 3 and 4 are fixed by the PR #3d brief. 1 is this program's own catch-all
/// for problems it detects before any of those stages is reached, and is also
/// what `clap` uses for a malformed command line.
mod exit {
    /// The command did what it was asked.
    pub const SUCCESS: std::process::ExitCode = std::process::ExitCode::SUCCESS;
    /// The export could not be read or parsed.
    pub const ADAPTER: u8 = 2;
    /// Parsed captures could not be normalized.
    pub const NORMALIZER: u8 = 3;
    /// The database could not be opened, migrated, or written.
    pub const DATABASE: u8 = 4;
    /// The command line asked for something this program does not do.
    pub const USAGE: u8 = 1;
}

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
    Import {
        provider: String,
        path: PathBuf,
        #[arg(long, default_value = "~/.kinexfluence/kinexfluence.db")]
        db: String,
    },
}

/// Why a command failed, and the exit code that carries the meaning.
#[derive(Debug)]
enum CliError {
    /// The file could not be read, or the adapter could not parse it.
    Adapter(String),
    /// The captures parsed, but could not be turned into rows.
    Normalizer(String),
    /// The database could not be opened, migrated, or written.
    Database(String),
    /// No adapter exists for the requested provider.
    UnknownProvider(String),
}

impl CliError {
    fn code(&self) -> u8 {
        match self {
            Self::Adapter(_) => exit::ADAPTER,
            Self::Normalizer(_) => exit::NORMALIZER,
            Self::Database(_) => exit::DATABASE,
            Self::UnknownProvider(_) => exit::USAGE,
        }
    }
}

/// Written out rather than derived: `kfx-cli` deliberately has no `thiserror`
/// dependency, and the brief's allowlist covers dev-dependencies only.
impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Adapter(message) => write!(f, "adapter error: {message}"),
            Self::Normalizer(message) => write!(f, "normalizer error: {message}"),
            Self::Database(message) => write!(f, "database error: {message}"),
            Self::UnknownProvider(provider) => {
                write!(f, "unsupported provider: {provider} (try \"chatgpt\")")
            }
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt().with_env_filter("info").init();

    match run(Cli::parse()).await {
        Ok(()) => exit::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.code())
        }
    }
}

async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.cmd {
        Cmd::Migrate { db } => migrate(&db).await,
        Cmd::Import { provider, path, db } => import(&provider, &path, &db).await,
    }
}

async fn migrate(db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    println!("migrations applied to {}", path.display());
    Ok(())
}

/// Read, parse, normalize, then write.
///
/// The order is deliberate: everything that can fail on the *input* happens
/// before the database is opened. A mistyped path or a corrupt export should
/// not leave a freshly created, empty database behind, and it should exit with
/// the adapter code rather than a database one.
async fn import(provider: &str, path: &Path, db: &str) -> Result<(), CliError> {
    let bytes = std::fs::read(path)
        .map_err(|error| CliError::Adapter(format!("reading {}: {error}", path.display())))?;

    let captures = match provider {
        // `ChatGptAdapter` is a unit struct, so there is nothing to construct.
        // `parse_export` comes from `ProviderAdapter`, which is in scope above.
        "chatgpt" => ChatGptAdapter
            .parse_export(&bytes)
            .map_err(|error| CliError::Adapter(error.to_string()))?,
        other => return Err(CliError::UnknownProvider(other.to_owned())),
    };

    let batch = kfx_core::normalizer::normalize(&captures)
        .map_err(|error| CliError::Normalizer(error.to_string()))?;

    let db_path = shellexpand(db);
    let config = storage_config(&db_path);

    let pool = kfx_core::storage::open(&db_path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    // Importing into an out-of-date schema would fail in confusing ways
    // halfway through, and running migrations is idempotent.
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    let summary = kfx_core::writer::write_batch(&pool, batch)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    print_summary(path, &summary);
    Ok(())
}

fn print_summary(path: &Path, summary: &ImportSummary) {
    println!("Imported from {}:", path.display());
    println!(
        "  Conversations: {} inserted, {} skipped (already present)",
        summary.conversations_inserted, summary.conversations_skipped
    );
    println!("  Messages:      {}", summary.messages_inserted);
    println!("  Snippets:      {}", summary.snippets_inserted);
    // `attachments_inserted` counts every attachment row, missing files
    // included, so "present" is the difference rather than the total.
    println!(
        "  Attachments:   {} present, {} missing",
        summary.attachments_present(),
        summary.attachments_missing
    );
}

fn storage_config(path: &Path) -> kfx_core::StorageConfig {
    kfx_core::StorageConfig {
        path: path.to_string_lossy().to_string(),
        use_encryption: false,
    }
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
