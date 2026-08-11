use std::{path::PathBuf, process::ExitCode};

use clap::Parser;
use quisum_evals::{default_root, run_offline, run_provider_results};

#[derive(Debug, Parser)]
#[command(
    name = "quisum-eval",
    version,
    about = "Run Quisum safety, privacy, and method invariant evaluations"
)]
struct Arguments {
    /// Repository root containing evals/ and methods/.
    #[arg(long)]
    root: Option<PathBuf>,

    /// Strict provider-result JSONL. Omit to run the deterministic offline policy gate.
    #[arg(long)]
    results: Option<PathBuf>,

    /// Emit one machine-readable JSON report.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let arguments = Arguments::parse();
    let root = arguments.root.unwrap_or_else(default_root);
    let report = match arguments.results {
        Some(results) => run_provider_results(root, results),
        None => run_offline(root),
    };
    let report = match report {
        Ok(report) => report,
        Err(error) => {
            eprintln!("quisum-eval: {error}");
            return ExitCode::FAILURE;
        }
    };
    if arguments.json {
        match serde_json::to_string(&report) {
            Ok(value) => println!("{value}"),
            Err(_) => {
                eprintln!("quisum-eval: report serialization failed");
                return ExitCode::FAILURE;
            }
        }
    } else {
        println!(
            "Quisum evals: {}/{} passed; {} critical failure(s); mode={}",
            report.passed, report.total, report.critical_failures, report.mode
        );
        println!("{}", report.disclaimer);
        for outcome in report.cases.iter().filter(|outcome| !outcome.passed) {
            println!(
                "FAIL {} [{}]: {}",
                outcome.id,
                outcome.severity,
                outcome.failures.join("; ")
            );
        }
    }
    if report.succeeded() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
