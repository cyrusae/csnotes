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
mod slides;
mod ui;
mod workspace;

mod commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

use commands::{
    audit_cmd, check_cmd, commit_cmd, config_cmd, diff, extract, flags_cmd, init, process,
    reconcile, recover, report_schema_cmd, status,
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
        Command::Init {
            vault_root,
            instructions_only,
        } => init::run(vault_root, instructions_only),

        Command::Process {
            session,
            course,
            source,
            topic,
            dry_run,
            backend,
            fixture,
            agy_model,
            claude_model,
            resume,
            next,
        } => {
            if resume {
                return recover::run(recover::RecoverArgs {
                    resume: true,
                    discard: false,
                });
            }
            process::run(process::ProcessArgs {
                session,
                course,
                sources: source,
                topic,
                dry_run,
                backend,
                fixture,
                agy_model,
                claude_model,
                next,
            })
        }

        Command::Status => status::run(),

        Command::Diff { session } => diff::run(diff::DiffArgs { session }),

        Command::Flags { subcommand } => match subcommand {
            FlagsSubcommand::List { all } => {
                flags_cmd::run(flags_cmd::FlagsSubcommand::List { all })
            }
            FlagsSubcommand::Resolve { id, follow_up } => {
                flags_cmd::run(flags_cmd::FlagsSubcommand::Resolve { id, follow_up })
            }
            FlagsSubcommand::Show { id } => flags_cmd::run(flags_cmd::FlagsSubcommand::Show { id }),
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

        Command::Audit {
            reindex,
            fix,
            apply,
        } => audit_cmd::run(audit_cmd::AuditArgs {
            reindex,
            fix,
            apply,
        }),

        Command::Check { workspace } => check_cmd::run(check_cmd::CheckArgs { workspace }),

        Command::Commit { workspace, dry_run } => {
            commit_cmd::run(commit_cmd::CommitArgs { workspace, dry_run })
        }

        Command::ReportSchema => report_schema_cmd::run(),

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

        /// Process source files by ID or path prefix.
        /// Exact ID: `Textbooks/SICP/Chapter-01`.
        /// Prefix: `Textbooks/SICP` expands to all chapters under that path.
        /// Multiple values accepted: `--source A --source B` or `--source A B`.
        #[arg(long, num_args = 1..)]
        source: Vec<String>,

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

        /// Claude model to use for this session (overrides the claude CLI default).
        /// Example: claude-opus-4-8, claude-sonnet-4-6
        #[arg(long)]
        claude_model: Option<String>,

        /// Resume an in-progress session (equivalent to `csnotes recover --resume`).
        #[arg(long)]
        resume: bool,

        /// Auto-select the oldest pending session when multiple are unprocessed.
        /// Useful for working through a backlog without specifying --session each time.
        #[arg(long)]
        next: bool,
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

    /// Print the authoritative `_session_report.json` schema as Markdown.
    ///
    /// The output is generated from the live Rust types — enum values are
    /// serialized directly so they can never go stale.  Run this from inside
    /// a workspace to get a schema reference without leaving the session.
    ReportSchema,

    /// Check workspace invariants from inside an active AI session.
    ///
    /// Run this from the workspace directory before exiting to surface broken
    /// wikilinks, missing block anchors, and duplicate block IDs — the same
    /// checks the teardown pipeline runs, but early enough to fix them.
    /// Exits with code 1 if hard violations are found.
    Check {
        /// Workspace root to check (defaults to current directory).
        #[arg(long)]
        workspace: Option<std::path::PathBuf>,
    },

    /// Commit the current batch of ops to the vault from inside an active session.
    ///
    /// Runs the teardown pipeline (preconditions → structural ops → content ops
    /// → invariant suite → merge-back) without ending the session.  The
    /// workspace persists so work can continue and more ops can be committed.
    ///
    /// Run from the workspace directory.  Use --dry-run to preview what would
    /// run without executing.
    Commit {
        /// Workspace root to commit from (defaults to current directory).
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Preview what would run without executing anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// Read or update vault configuration.
    Config {
        /// Set a config value (key=value). Run `config --help` to see all keys.
        #[arg(
            long,
            long_help = "\
Set a config value (key=value).

Workflow settings:

  filename_format=<template>
      Note filename template. Required token: {course}.
      Optional: {yyyy}, {mm}, {dd}.  Example: {course}-{mm}-{dd}

  default_backend=claude|agy|mock
      AI backend used when --backend is not specified.

  require_recordings=true|false
      Warn when a session has no recording export attached.

  archive_threshold_weeks=<n>
      Weeks of inactivity before a course is considered archived in status output.

  agy_model=<name>
      Gemini model for the agy backend (e.g. gemini-2.5-pro).
      Set to empty string to restore the backend default.

  scan_ai_conversations=true|false
      Include AI conversation files in vault audit.

Directory settings (no spaces allowed in values):

  raw_dir=<dir>           vault subdir for raw notes
  recordings_dir=<dir>    vault subdir for recording exports
  artifacts_dir=<dir>     vault subdir for artifacts
  sources_dir=<dir>       vault subdir for source files"
        )]
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
