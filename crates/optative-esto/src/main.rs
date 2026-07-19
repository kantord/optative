use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Parser, Subcommand};
use esto::run_file;
use esto::watch::{WatchTrigger, watch_file};

#[derive(Parser)]
#[command(
    name = "esto",
    about = "Run declarative .op.tsx/.eso.jsx reconciler scripts."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a reconciler script once: diff observed vs. desired state, call
    /// enter/update/exit for the delta.
    Run {
        /// Path to the .op.tsx/.op.jsx/.eso.jsx script.
        file: String,
        /// Compute the diff and print it without calling enter/update/exit.
        #[arg(long)]
        dry_run: bool,
        /// Suppress the [enter]/[update]/[exit] log lines and the summary.
        #[arg(long)]
        quiet: bool,
    },
    /// Re-run a script whenever a trigger fires.
    Watch {
        /// Path to the .op.tsx/.op.jsx/.eso.jsx script.
        file: String,
        /// Trigger to re-run on: git-commit, inotify:<path>, or fs:<path>. Repeatable.
        #[arg(long = "on", value_parser = parse_trigger)]
        triggers: Vec<WatchTrigger>,
        /// Also re-run on a fixed interval (e.g. 5s, 100ms, 1m), independent of triggers.
        #[arg(long, value_parser = parse_duration)]
        every: Option<Duration>,
        /// Compute the diff and print it without calling enter/update/exit.
        #[arg(long)]
        dry_run: bool,
        /// Suppress the [enter]/[update]/[exit] log lines.
        #[arg(long)]
        quiet: bool,
    },
    /// Write esto's TypeScript ambient types (esto.d.ts + tsconfig.esto.json).
    Types {
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Like `types`, but also runs `tsc --noEmit` against the generated tsconfig.
    TypeCheck {
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
}

fn parse_trigger(s: &str) -> Result<WatchTrigger, String> {
    if s == "git-commit" {
        Ok(WatchTrigger::GitCommit)
    } else if let Some(path) = s.strip_prefix("inotify:").or_else(|| s.strip_prefix("fs:")) {
        Ok(WatchTrigger::FsPath(PathBuf::from(path)))
    } else {
        Err(format!(
            "unknown trigger '{s}'; use inotify:<path>, fs:<path>, or git-commit"
        ))
    }
}

fn parse_duration(s: &str) -> Result<Duration, String> {
    if let Some(rest) = s.strip_suffix("ms") {
        rest.parse::<u64>()
            .map(Duration::from_millis)
            .map_err(|e| e.to_string())
    } else if let Some(rest) = s.strip_suffix('s') {
        rest.parse::<u64>()
            .map(Duration::from_secs)
            .map_err(|e| e.to_string())
    } else if let Some(rest) = s.strip_suffix('m') {
        rest.parse::<u64>()
            .map(|n| Duration::from_secs(n * 60))
            .map_err(|e| e.to_string())
    } else {
        Err(format!(
            "invalid duration '{s}'; expected e.g. '5s', '100ms', '1m'"
        ))
    }
}

fn write_or_exit(path: &Path, contents: impl AsRef<[u8]>) {
    if let Err(e) = std::fs::write(path, contents) {
        eprintln!("esto types: failed to write {}: {e}", path.display());
        std::process::exit(1);
    }
    eprintln!("esto types: wrote {}", path.display());
}

fn cmd_types(out: &Path, also_check: bool) {
    if let Err(e) = std::fs::create_dir_all(out) {
        eprintln!("esto types: could not create output directory: {e}");
        std::process::exit(1);
    }
    let dts_dest = out.join("esto.d.ts");
    write_or_exit(&dts_dest, esto::types::ESTO_DTS);
    let tsconfig_dest = out.join("tsconfig.esto.json");
    write_or_exit(&tsconfig_dest, esto::types::ESTO_TSCONFIG);

    if also_check {
        let status = std::process::Command::new("tsc")
            .arg("--noEmit")
            .arg("--project")
            .arg(&tsconfig_dest)
            .status()
            .unwrap_or_else(|e| {
                eprintln!("esto type-check: could not run tsc — is TypeScript installed? ({e})");
                eprintln!("  Install with: npm install -g typescript");
                std::process::exit(127);
            });
        std::process::exit(status.code().unwrap_or(1));
    }
}

fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Command::Run {
            file,
            dry_run,
            quiet,
        } => {
            if let Err(e) = run_file(&file, dry_run, quiet) {
                eprintln!("esto run: {file}\n\n{e}");
                std::process::exit(1);
            }
        }
        Command::Watch {
            file,
            triggers,
            every,
            dry_run,
            quiet,
        } => {
            if let Err(e) = watch_file(&file, triggers, every, dry_run, quiet) {
                eprintln!("esto watch: {e}");
                std::process::exit(1);
            }
        }
        Command::Types { out } => cmd_types(&out, false),
        Command::TypeCheck { out } => cmd_types(&out, true),
    }
}
