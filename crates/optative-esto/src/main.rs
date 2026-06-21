use std::time::Duration;

use clap::Parser;
use optative_esto::{ReconcileConfig, run, run_file};
use tracing_subscriber;

#[derive(Parser)]
#[command(
    name = "esto",
    about = "Continuously reconcile a desired set against current state, running hook scripts on changes.",
    long_about = "esto loops forever: it runs --to to get what should exist, runs --from to get what does \
exist, then calls your worker scripts for items that appeared (--enter), disappeared (--exit), or changed \
(--update). At least one worker is required.\n\n\
I/O format — --from and --to scripts emit one line per item:\n  \
  key<TAB>value\n\
The value is opaque — a hash, a JSON blob, a version string, anything single-line. esto never parses it; \
workers receive it verbatim. Change detection is simple string equality: --update only fires when the \
value differs between --from and --to.\n\n\
Worker protocol (default — simple mode):\n  \
Workers are invoked once per item: cmd key [value [old_value new_value]]\n  \
Exit 0 = success; nonzero = error. No stdin/stdout protocol required.\n\n\
Worker protocol (--stateful mode):\n  \
Workers are long-lived processes. esto writes one line per task on stdin:\n  \
  enter/exit:  key<TAB>value\n  \
  update:      key<TAB>old_value<TAB>new_value\n\
Workers must respond on stdout: done<TAB>key  |  error<TAB>key<TAB>msg  |  shutdown"
)]
struct Cli {
    /// Shell command emitting current world state as TSV: one "key<TAB>value" line per item.
    /// Optional: omit to start with an empty state (everything in --to will trigger --enter).
    /// In --once mode this seeds the initial state. In loop mode, used only for --reingest-every.
    #[arg(long)]
    from: Option<String>,

    /// Shell command emitting desired state as TSV: one "key<TAB>value" line per item.
    /// Runs every loop iteration — can be a script, not just 'cat file.tsv'.
    #[arg(long)]
    to: String,

    /// Worker invoked for each new item. Simple mode (default): cmd key value.
    /// Stateful mode (--stateful): long-lived process receiving key<TAB>value on stdin.
    #[arg(long)]
    enter: Option<String>,

    /// Worker invoked for each removed item. Simple mode: cmd key value.
    #[arg(long)]
    exit: Option<String>,

    /// Worker invoked for each changed item. Simple mode: cmd key old_value new_value.
    #[arg(long)]
    update: Option<String>,

    /// Minimum pause between loop iterations (e.g. 5s, 100ms, 1m).
    #[arg(long, value_parser = parse_duration)]
    rate_limit: Option<Duration>,

    /// Re-read --from every N iterations to sync internal state with actual world state.
    #[arg(long)]
    reingest_every: Option<u64>,

    /// Run exactly one reconcile cycle then exit (CI/script mode).
    /// Prints a summary line: "reconciled: N enter, N update, N exit (N unchanged)".
    #[arg(long)]
    once: bool,

    /// Use long-lived worker processes with stdin/stdout protocol instead of per-item invocation.
    /// Workers receive tasks on stdin and must reply done<TAB>key / error<TAB>key<TAB>msg / shutdown.
    #[arg(long)]
    stateful: bool,

    /// Suppress per-event log lines ([enter]/[exit]/[update]) and the --once summary.
    #[arg(long)]
    quiet: bool,

    /// Show what would happen without dispatching any workers.
    #[arg(long)]
    dry_run: bool,
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
        Err(format!("invalid duration '{s}'; expected e.g. '5s', '100ms', '1m'"))
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    // Intercept `esto run [--dry-run] [--quiet] <file>` before clap sees the args,
    // so the existing flat flag interface stays backward-compatible.
    let raw: Vec<String> = std::env::args().skip(1).collect();
    if raw.first().map(|s| s == "run").unwrap_or(false) {
        let rest = &raw[1..];
        let mut dry_run = false;
        let mut quiet = false;
        let mut file: Option<String> = None;
        for arg in rest {
            match arg.as_str() {
                "--dry-run" => dry_run = true,
                "--quiet" => quiet = true,
                _ if !arg.starts_with('-') => file = Some(arg.clone()),
                other => {
                    eprintln!("esto run: unknown flag {other}");
                    std::process::exit(1);
                }
            }
        }
        let file = file.unwrap_or_else(|| {
            eprintln!("esto run: missing file argument\nUsage: esto run [--dry-run] [--quiet] <file.mjs>");
            std::process::exit(1);
        });
        if let Err(e) = run_file(&file, dry_run, quiet) {
            eprintln!("esto run: {e}");
            std::process::exit(1);
        }
        return;
    }

    let cli = Cli::parse();

    if cli.enter.is_none() && cli.exit.is_none() && cli.update.is_none() && !cli.dry_run {
        eprintln!("esto: at least one of --enter, --exit, --update is required (or use --dry-run)");
        std::process::exit(1);
    }

    let config = ReconcileConfig {
        from: cli.from,
        to: cli.to,
        enter: cli.enter,
        exit: cli.exit,
        update: cli.update,
        rate_limit: cli.rate_limit,
        reingest_every: cli.reingest_every,
        once: cli.once,
        stateful: cli.stateful,
        quiet: cli.quiet,
        dry_run: cli.dry_run,
    };

    if let Err(e) = run(config) {
        eprintln!("esto: {e}");
        std::process::exit(1);
    }
}
