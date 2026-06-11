mod audit;
mod backend;
mod config;
mod error;
mod flags;
mod frontmatter;
mod manifest;
mod markdown;
mod obsidian;
mod ops;
mod pathutil;
mod report;
mod ui;
mod workspace;

mod commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use commands::{
    audit_cmd, config_cmd, diff, extract, flags_cmd, init, process, recover, reconcile, status,
};
use config::AiBackend;

fn main() {
    crate::ui::init_color();
    let cli = Cli::parse();
    let result = dispatch(cli);
    if let Err(e) = result {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}

fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Init { vault_root, instructions_only } => init::run(vault_root, instructions_only),

        Command::Process {
            session,
            course,
            source,
            topic,
            dry_run,
            backend,
            fixture,
            agy_model,
        } => process::run(process::ProcessArgs {
            session,
            course,
            source,
            topic,
            dry_run,
            backend,
            fixture,
            agy_model,
        }),

        Command::Status => status::run(),

        Command::Diff { session } => diff::run(diff::DiffArgs { session }),

        Command::Flags { subcommand } => match subcommand {
            FlagsSubcommand::List { all } => {
                flags_cmd::run(flags_cmd::FlagsSubcommand::List { all })
            }
            FlagsSubcommand::Resolve { id, follow_up } => {
                flags_cmd::run(flags_cmd::FlagsSubcommand::Resolve { id, follow_up })
            }
            FlagsSubcommand::Show { id } => {
                flags_cmd::run(flags_cmd::FlagsSubcommand::Show { id })
            }
        },

        Command::Extract {
            session,
            kind,
            stdout,
        } => extract::run(extract::ExtractArgs {
            session,
            kind,
            stdout,
        }),

        Command::Reconcile {
            notify,
            rename_spaces,
        } => reconcile::run(reconcile::ReconcileArgs {
            notify,
            rename_spaces,
            quiet: false,
        }),

        Command::Recover { resume, discard } => {
            recover::run(recover::RecoverArgs { resume, discard })
        }

        Command::Audit { reindex, fix, apply } => {
            audit_cmd::run(audit_cmd::AuditArgs { reindex, fix, apply })
        }

        Command::Config {
            set,
            show,
            add_course,
            archive,
            migrate,
            apply,
        } => config_cmd::run(config_cmd::ConfigArgs {
            set,
            show,
            add_course,
            archive,
            migrate,
            apply,
        }),
    }
}

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "csnotes",
    about = "AI-assisted synthesis of graduate lecture notes",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold a new vault with directory structure and instruction files.
    Init {
        /// Path for the vault root (defaults to current directory).
        #[arg(long)]
        vault_root: Option<PathBuf>,
        /// Only write instruction files into an existing vault; skip
        /// directory scaffolding and config prompts.
        #[arg(long)]
        instructions_only: bool,
    },

    /// Run an AI synthesis session against pending notes.
    Process {
        /// Process a specific session by date (e.g. 2026-07-28 or 07-28).
        #[arg(long)]
        session: Option<String>,

        /// Restrict to a specific course.
        #[arg(long)]
        course: Option<String>,

        /// Process a source file by ID (e.g. SICP/SICP-ch01).
        #[arg(long)]
        source: Option<String>,

        /// Study/review session focused on an existing topic (no new input).
        #[arg(long)]
        topic: Option<String>,

        /// Print what would happen without launching or mutating anything.
        #[arg(long)]
        dry_run: bool,

        /// AI backend to use (overrides config default_backend).
        #[arg(short = 'm', long = "backend", value_enum)]
        backend: Option<AiBackend>,

        /// Fixture set name for --backend mock.
        #[arg(long, hide = true)]
        fixture: Option<String>,

        /// Gemini model to use for this session (overrides agy_model in config).
        /// Example: gemini-2.5-flash, gemini-2.5-pro
        #[arg(long)]
        agy_model: Option<String>,
    },

    /// Show processing status for sessions, sources, topics, and flags.
    Status,

    /// Show a semantic diff of the last session's changes.
    Diff {
        #[arg(long)]
        session: Option<String>,
    },

    /// Manage review flags.
    Flags {
        #[command(subcommand)]
        subcommand: FlagsSubcommand,
    },

    /// Extract action items, deadlines, and questions from raw notes.
    Extract {
        #[arg(long)]
        session: Option<String>,
        /// Filter by type: actions | deadlines | questions
        #[arg(long = "type")]
        kind: Option<String>,
        #[arg(long)]
        stdout: bool,
    },

    /// Scan for new files and register them in the manifest.
    Reconcile {
        /// Emit a desktop notification if new files are found.
        #[arg(long)]
        notify: bool,
        /// Rename space-containing filenames (hyphens | underscores).
        /// Without this flag, spaces are flagged but not renamed.
        #[arg(long, value_name = "STYLE")]
        rename_spaces: Option<String>,
    },

    /// Resume or discard an in-progress session after a crash.
    Recover {
        /// Resume the interrupted session.
        #[arg(long)]
        resume: bool,
        /// Discard the workspace without resuming.
        #[arg(long)]
        discard: bool,
    },

    /// Run vault invariant checks; optionally rebuild the manifest.
    Audit {
        /// Rebuild csnotes.json from frontmatter + filesystem.
        #[arg(long)]
        reindex: bool,
        /// Show mechanical repairs without writing anything (dry-run).
        /// Combine with --apply to execute.
        #[arg(long)]
        fix: bool,
        /// Execute the repairs shown by --fix.
        #[arg(long)]
        apply: bool,
    },

    /// Read or update vault configuration.
    Config {
        /// Set a config value (key=value).
        #[arg(long)]
        set: Option<String>,
        /// Print current configuration.
        #[arg(long)]
        show: bool,
        /// Add a course to active_courses.
        #[arg(long)]
        add_course: Option<String>,
        /// Archive a course (removes from active_courses).
        #[arg(long)]
        archive: Option<String>,
        /// Show the rename plan for migrating files to the current
        /// filename_format (dry-run).  Combine with --apply to execute.
        #[arg(long)]
        migrate: bool,
        /// Execute the rename plan shown by --migrate.
        #[arg(long)]
        apply: bool,
    },
}

#[derive(Subcommand)]
enum FlagsSubcommand {
    /// List open actionable flags (add --all for threads and changelog).
    List {
        #[arg(long)]
        all: bool,
    },
    /// Mark a flag as resolved.
    Resolve {
        id: String,
        /// Optional follow-up note to record at resolution.
        #[arg(long)]
        follow_up: Option<String>,
    },
    /// Show full details for a flag.
    Show { id: String },
}
