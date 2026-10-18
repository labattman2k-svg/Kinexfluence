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
    /// A project command was given something it cannot use.
    ///
    /// Shares its value with [`ADAPTER`]: from the caller's side both mean "the
    /// input was wrong", and scripts react to the number, not the noun.
    pub const INVALID_INPUT: u8 = 2;
    /// A project lookup found nothing.
    ///
    /// Shares its value with [`NORMALIZER`] for the same reason.
    pub const NOT_FOUND: u8 = 3;
}

/// Where the database lives unless the caller says otherwise.
const DEFAULT_DB: &str = "~/.kinexfluence/kinexfluence.db";

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
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
    /// Import a provider export file.
    Import {
        provider: String,
        path: PathBuf,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
    /// Create, list, and inspect projects.
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Run the HTTP API server.
    ///
    /// Mostly for headless use and E2E tests: the desktop shell starts the
    /// server itself. Runs until interrupted.
    Serve {
        #[arg(long, default_value_t = 47821)]
        port: u16,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
    /// Browse and manage snippets.
    Snippets {
        #[command(subcommand)]
        cmd: SnippetsCmd,
    },
    /// Manage project carts.
    Cart {
        #[command(subcommand)]
        cmd: CartCmd,
    },
}

/// The `project` subcommands.
#[derive(Subcommand)]
enum ProjectCmd {
    /// Create a project and print its id.
    Create {
        name: String,
        /// Used as-is when given; generated from the name otherwise.
        #[arg(long)]
        slug: Option<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        brief: Option<String>,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
    /// List projects.
    List {
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
    /// Show one project by id or slug.
    Show {
        id_or_slug: String,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
    /// Change a project's status.
    Status {
        id_or_slug: String,
        status: String,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
}

/// The `snippets` subcommands.
#[derive(Subcommand)]
enum SnippetsCmd {
    /// List snippets with pagination.
    List {
        #[arg(long, default_value_t = 50)]
        limit: i64,
        #[arg(long, default_value_t = 0)]
        offset: i64,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
}

/// The `cart` subcommands.
#[derive(Subcommand)]
enum CartCmd {
    /// Add a snippet to a project's cart.
    Add {
        snippet_id: String,
        #[arg(long)]
        project: String,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
    /// List items in a project's cart.
    List {
        #[arg(long)]
        project: String,
        #[arg(long, default_value = DEFAULT_DB)]
        db: String,
    },
    /// Remove an item from a project's cart.
    Remove {
        item_id: String,
        #[arg(long)]
        project: String,
        #[arg(long, default_value = DEFAULT_DB)]
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
    /// A project command was given something it cannot use.
    InvalidInput(String),
    /// A project lookup found nothing.
    NotFound(String),
}

impl CliError {
    fn code(&self) -> u8 {
        match self {
            Self::Adapter(_) => exit::ADAPTER,
            Self::InvalidInput(_) => exit::INVALID_INPUT,
            Self::Normalizer(_) => exit::NORMALIZER,
            Self::NotFound(_) => exit::NOT_FOUND,
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
            Self::InvalidInput(message) => write!(f, "invalid input: {message}"),
            Self::NotFound(message) => write!(f, "not found: {message}"),
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
        Cmd::Project { cmd } => project(cmd).await,
        Cmd::Serve { port, db } => serve(port, &db).await,
        Cmd::Snippets { cmd } => snippets(cmd).await,
        Cmd::Cart { cmd } => cart(cmd).await,
    }
}

/// Run the API server in the foreground until interrupted.
///
/// `kfx_core::server::serve` owns everything else: it finds a free port if the
/// requested one is taken, opens and migrates the database, and serves. This
/// only supplies the configuration.
async fn serve(port: u16, db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);

    let config = kfx_core::Config {
        server: kfx_core::ServerConfig {
            // Loopback only, per §5. This is not configurable through the CLI.
            host: "127.0.0.1".to_owned(),
            port,
        },
        storage: storage_config(&path),
    };

    kfx_core::server::serve(config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))
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

// ---------------------------------------------------------------------------
// project
// ---------------------------------------------------------------------------

async fn project(cmd: ProjectCmd) -> Result<(), CliError> {
    match cmd {
        ProjectCmd::Create {
            name,
            slug,
            description,
            brief,
            db,
        } => project_create(&name, slug, description, brief, &db).await,
        ProjectCmd::List { status, db } => project_list(status, &db).await,
        ProjectCmd::Show { id_or_slug, db } => project_show(&id_or_slug, &db).await,
        ProjectCmd::Status {
            id_or_slug,
            status,
            db,
        } => project_status(&id_or_slug, &status, &db).await,
    }
}

/// Map a domain error onto the CLI's exit-code taxonomy.
fn project_error(error: kfx_core::project::ProjectError) -> CliError {
    use kfx_core::project::ProjectError;

    match error {
        ProjectError::Db(error) => CliError::Database(error.to_string()),
        ProjectError::NotFound(message) => CliError::NotFound(message),
        ProjectError::InvalidSlug(message) => CliError::InvalidInput(message),
        ProjectError::EmptyName => CliError::InvalidInput("project name is required".to_owned()),
        ProjectError::DuplicateSlug(message) => {
            CliError::InvalidInput(format!("duplicate slug: {message}"))
        }
        ProjectError::InvalidStatus(message) => CliError::InvalidInput(message),
    }
}

async fn project_create(
    name: &str,
    slug: Option<String>,
    description: Option<String>,
    brief: Option<String>,
    db: &str,
) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    let created = kfx_core::project::create(
        &pool,
        kfx_core::project::CreateProject {
            name: name.to_owned(),
            slug,
            description,
            brief,
        },
    )
    .await
    .map_err(project_error)?;

    // The id alone goes to stdout so `PID=$(kinexfluence project create ...)`
    // works. Everything a human would want to read goes to stderr.
    println!("{}", created.id);
    eprintln!(
        "created project {:?} (slug: {}, status: {})",
        created.name, created.slug, created.status
    );

    Ok(())
}

async fn project_list(status: Option<String>, db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    let projects = match status.as_deref() {
        Some(status) => kfx_core::project::list_by_status(&pool, status).await,
        None => kfx_core::project::list(&pool).await,
    }
    .map_err(project_error)?;

    if projects.is_empty() {
        // Not an error: an empty list is a valid answer, and exiting 3 for it
        // would make "no projects yet" indistinguishable from "no such
        // project".
        eprintln!("no projects");
        return Ok(());
    }

    println!("{:<38} {:<23} {:<10} NAME", "ID", "SLUG", "STATUS");
    for project in &projects {
        println!(
            "{:<38} {:<23} {:<10} {}",
            project.id, project.slug, project.status, project.name
        );
    }

    Ok(())
}

async fn project_show(id_or_slug: &str, db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    let project = kfx_core::project::resolve(&pool, id_or_slug)
        .await
        .map_err(project_error)?
        .ok_or_else(|| CliError::NotFound(id_or_slug.to_owned()))?;

    print_project(&project);
    Ok(())
}

async fn project_status(id_or_slug: &str, status: &str, db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    let project = kfx_core::project::resolve(&pool, id_or_slug)
        .await
        .map_err(project_error)?
        .ok_or_else(|| CliError::NotFound(id_or_slug.to_owned()))?;

    kfx_core::project::update_status(&pool, &project.id, status)
        .await
        .map_err(project_error)?;

    eprintln!(
        "project {} ({}) is now {}",
        project.slug, project.id, status
    );
    Ok(())
}

fn print_project(project: &kfx_core::project::Project) {
    println!("Project: {}", project.name);
    println!("  ID:          {}", project.id);
    println!("  Slug:        {}", project.slug);
    println!("  Status:      {}", project.status);
    println!(
        "  Description: {}",
        project.description.as_deref().unwrap_or("-")
    );
    println!("  Brief:       {}", project.brief.as_deref().unwrap_or("-"));
    // Fixed width so the column does not jump between rows.
    println!(
        "  Created:     {}",
        project.created_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "  Updated:     {}",
        project.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
    );
}

// ---------------------------------------------------------------------------
// snippets
// ---------------------------------------------------------------------------

async fn snippets(cmd: SnippetsCmd) -> Result<(), CliError> {
    match cmd {
        SnippetsCmd::List { limit, offset, db } => snippets_list(limit, offset, &db).await,
    }
}

async fn snippets_list(limit: i64, offset: i64, db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    let snippets = kfx_core::snippet::list(&pool, Some(limit), Some(offset))
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    if snippets.is_empty() {
        eprintln!("no snippets");
        return Ok(());
    }

    println!("{:<38} {:<8} KIND", "ID", "TYPE");
    for snippet in &snippets {
        println!(
            "{:<38} {:<8} {}",
            snippet.id,
            snippet.kind,
            snippet.text.lines().next().unwrap_or("")
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// cart
// ---------------------------------------------------------------------------

async fn cart(cmd: CartCmd) -> Result<(), CliError> {
    match cmd {
        CartCmd::Add {
            snippet_id,
            project,
            db,
        } => cart_add(&snippet_id, &project, &db).await,
        CartCmd::List { project, db } => cart_list(&project, &db).await,
        CartCmd::Remove {
            item_id,
            project,
            db,
        } => cart_remove(&item_id, &project, &db).await,
    }
}

fn cart_error(error: kfx_core::cart::CartError) -> CliError {
    use kfx_core::cart::CartError;

    match error {
        CartError::SnippetNotFound(message) => CliError::NotFound(message),
        CartError::ProjectNotFound(message) => CliError::NotFound(message),
        CartError::Database(error) => CliError::Database(error.to_string()),
    }
}

async fn cart_add(snippet_id: &str, project_id: &str, db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    let item_id = kfx_core::cart::add_to_cart(&pool, project_id, snippet_id)
        .await
        .map_err(cart_error)?;

    println!("{}", item_id);
    eprintln!("added snippet {} to cart", snippet_id);
    Ok(())
}

async fn cart_list(project_id: &str, db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    let items = kfx_core::cart::list_cart(&pool, project_id)
        .await
        .map_err(cart_error)?;

    if items.is_empty() {
        eprintln!("cart is empty");
        return Ok(());
    }

    println!("{:<38} {:<8} KIND", "ITEM ID", "TYPE");
    for item in &items {
        println!(
            "{:<38} {:<8} {}",
            item.item_id,
            item.kind,
            item.text.lines().next().unwrap_or("")
        );
    }

    Ok(())
}

async fn cart_remove(item_id: &str, project_id: &str, db: &str) -> Result<(), CliError> {
    let path = shellexpand(db);
    let config = storage_config(&path);

    let pool = kfx_core::storage::open(&path, &config)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;
    kfx_core::storage::migrate(&pool)
        .await
        .map_err(|error| CliError::Database(error.to_string()))?;

    kfx_core::cart::remove_from_cart(&pool, project_id, item_id)
        .await
        .map_err(cart_error)?;

    eprintln!("removed item {} from cart", item_id);
    Ok(())
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
