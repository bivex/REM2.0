use clap::{Parser, Subcommand};

/// REM3 — Extract-function refactoring for Rust
///
/// Performs semantics-preserving Extract Function refactorings with
/// borrow-checker repair and optional equivalence verification.
#[derive(Parser, Debug)]
#[command(
    name    = "rem",
    version,
    about   = "REM3: Extract Function refactoring for Rust",
    long_about = None,
)]
pub struct Cli {
    /// Set log level: error | warn | info | debug | trace
    #[arg(long, env = "REM_LOG", default_value = "info")]
    pub log_level: String,

    /// Emit logs as JSON (useful for CI / machine consumption).
    #[arg(long)]
    pub json_logs: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Extract a contiguous code fragment into a new function.
    Extract(ExtractArgs),
    /// Verify that two functions are behaviourally equivalent (requires CHARON + AENEAS).
    Verify(VerifyArgs),
}

// ── extract ──────────────────────────────────────────────────────────────────

/// `rem extract --file src/lib.rs --start 120 --end 240 --name compute_sum`
#[derive(clap::Args, Debug)]
pub struct ExtractArgs {
    /// Source file containing the selection.
    #[arg(short, long)]
    pub file: String,

    /// Byte offset of selection start (inclusive).
    #[arg(long)]
    pub start: u32,

    /// Byte offset of selection end (exclusive).
    #[arg(long)]
    pub end: u32,

    /// Name for the extracted function.
    #[arg(short, long)]
    pub name: String,

    /// Override the project root (directory with Cargo.toml).
    /// Auto-detected if omitted.
    #[arg(long)]
    pub project_root: Option<String>,

    /// Also run the CHARON/AENEAS equivalence verification pass.
    #[arg(long, default_value_t = false)]
    pub verify: bool,

    /// Output result as JSON instead of human-readable text.
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Path to the CHARON binary (required when --verify is set).
    #[arg(long, env = "REM_CHARON_BIN", default_value = "charon")]
    pub charon_bin: String,

    /// Path to the AENEAS binary (required when --verify is set).
    #[arg(long, env = "REM_AENEAS_BIN", default_value = "aeneas")]
    pub aeneas_bin: String,
}

// ── verify ───────────────────────────────────────────────────────────────────

/// `rem verify --file src/lib.rs --original big_fn --extracted small_fn`
#[derive(clap::Args, Debug)]
pub struct VerifyArgs {
    /// Source file containing both functions.
    #[arg(short, long)]
    pub file: String,

    /// Name of the original (pre-extraction) function.
    #[arg(long)]
    pub original: String,

    /// Name of the extracted function.
    #[arg(long)]
    pub extracted: String,

    /// Project root (directory with Cargo.toml).
    #[arg(long)]
    pub project_root: String,

    /// Output result as JSON.
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Path to the CHARON binary.
    #[arg(long, env = "REM_CHARON_BIN", default_value = "charon")]
    pub charon_bin: String,

    /// Path to the AENEAS binary.
    #[arg(long, env = "REM_AENEAS_BIN", default_value = "aeneas")]
    pub aeneas_bin: String,
}
