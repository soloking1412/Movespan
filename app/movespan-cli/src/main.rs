//! Movespan CLI: capture a Move workload's access sets, run the Block-STM
//! contention model, and report hotspots plus refactor suggestions.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use move_core_types::account_address::AccountAddress;

use movespan_core::{plan_calls, plan_init, replay, Network, ReplayConfig, Sandbox, WorkloadSpec};
use movespan_model::analyze;
use movespan_report::{to_html, to_json, to_text};
use movespan_rules::suggest;
use movespan_types::Workload;

#[derive(Parser)]
#[command(
    name = "movespan",
    version,
    about = "Block-STM contention profiler for Aptos Move"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Mode B: profile a compiled package under a synthetic workload.
    Analyze(PackageArgs),
    /// Mode A: replay historical transactions from a network.
    Replay(ReplayArgs),
    /// Mode B: report the raw storage footprint of a workload, without the model.
    Footprint(PackageArgs),
}

#[derive(Args)]
struct PackageArgs {
    /// Compiled Move package directory (the one containing `build/`).
    #[arg(long)]
    package: PathBuf,
    /// Address the package was compiled to; a funded account is created there
    /// to run the workload's init calls.
    #[arg(long)]
    module_address: String,
    /// Workload specification (TOML).
    #[arg(long)]
    workload: PathBuf,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Args)]
struct ReplayArgs {
    /// Network to fork: `mainnet`, `testnet`, `devnet`, or a custom node URL.
    #[arg(long)]
    network: String,
    #[arg(long)]
    start_version: u64,
    #[arg(long, default_value_t = 1000)]
    count: u64,
    #[command(flatten)]
    output: OutputArgs,
}

#[derive(Args)]
struct OutputArgs {
    /// Block-STM worker threads to model.
    #[arg(long, default_value_t = 32)]
    threads: usize,
    #[arg(long, value_enum, default_value_t = Format::Text)]
    format: Format,
    /// Write the report to a file instead of stdout.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Exit non-zero if parallelizability is below this (CI regression guard).
    #[arg(long)]
    min_score: Option<f64>,
}

#[derive(Copy, Clone, ValueEnum)]
enum Format {
    Text,
    Json,
    Html,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    match Cli::parse().command {
        Command::Analyze(args) => report(&capture_package(&args)?, &args.output),
        Command::Footprint(args) => {
            print!("{}", footprint(&capture_package(&args)?));
            Ok(ExitCode::SUCCESS)
        }
        Command::Replay(args) => {
            let config = ReplayConfig {
                network: parse_network(&args.network),
                start_version: args.start_version,
                count: args.count,
            };
            report(&replay(&config)?, &args.output)
        }
    }
}

fn capture_package(args: &PackageArgs) -> Result<Workload> {
    let spec: WorkloadSpec = toml::from_str(
        &fs::read_to_string(&args.workload)
            .with_context(|| format!("reading workload {}", args.workload.display()))?,
    )?;
    let module_address = AccountAddress::from_hex_literal(&args.module_address)
        .map_err(|e| anyhow!("invalid --module-address: {e}"))?;

    let modules = collect_package_modules(&args.package)?;
    if modules.is_empty() {
        bail!("no compiled modules found under {}", args.package.display());
    }

    let mut sandbox = Sandbox::new();
    let module_account = sandbox.account_at(module_address);
    let users = sandbox.create_accounts(spec.accounts);

    for path in modules {
        let code = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        sandbox.publish_module(code)?;
    }
    for txn in plan_init(&spec, &module_account, &users)? {
        sandbox.run(txn)?;
    }

    let mut accesses = Vec::with_capacity(spec.txns);
    for planned in plan_calls(&spec, &users)? {
        accesses.push(sandbox.run_and_capture(planned.txn)?);
    }

    Ok(Workload {
        txns: accesses,
        locations: sandbox.locations(),
    })
}

fn report(workload: &Workload, output: &OutputArgs) -> Result<ExitCode> {
    let analysis = analyze(workload, output.threads);
    let suggestions = suggest(workload, &analysis, 5);
    let rendered = match output.format {
        Format::Text => to_text(&analysis, &suggestions),
        Format::Json => to_json(&analysis, &suggestions),
        Format::Html => to_html(&analysis, &suggestions),
    };

    match &output.out {
        Some(path) => {
            fs::write(path, rendered).with_context(|| format!("writing {}", path.display()))?
        }
        None => print!("{rendered}"),
    }

    if let Some(min) = output.min_score {
        if analysis.parallelizability < min {
            eprintln!(
                "parallelizability {:.3} is below the {min:.3} threshold",
                analysis.parallelizability
            );
            return Ok(ExitCode::FAILURE);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn footprint(workload: &Workload) -> String {
    let mut reads: BTreeMap<_, usize> = BTreeMap::new();
    let mut writes: BTreeMap<_, usize> = BTreeMap::new();
    for txn in &workload.txns {
        for &location in &txn.reads {
            *reads.entry(location).or_default() += 1;
        }
        for &location in &txn.writes {
            *writes.entry(location).or_default() += 1;
        }
    }

    let mut ids: Vec<_> = reads.keys().chain(writes.keys()).copied().collect();
    ids.sort_unstable();
    ids.dedup();
    ids.sort_by_key(|id| std::cmp::Reverse(writes.get(id).copied().unwrap_or(0)));

    let mut out = String::new();
    let _ = writeln!(
        out,
        "STORAGE FOOTPRINT ({} txns, {} locations)",
        workload.txns.len(),
        ids.len()
    );
    for id in ids {
        let _ = writeln!(
            out,
            "  {:>6} reads  {:>6} writes  {}",
            reads.get(&id).copied().unwrap_or(0),
            writes.get(&id).copied().unwrap_or(0),
            workload.locations.label(id),
        );
    }
    out
}

fn parse_network(value: &str) -> Network {
    match value.to_lowercase().as_str() {
        "mainnet" => Network::Mainnet,
        "testnet" => Network::Testnet,
        "devnet" => Network::Devnet,
        _ => Network::Custom(value.to_string()),
    }
}

fn collect_package_modules(package: &Path) -> Result<Vec<PathBuf>> {
    let mut modules = Vec::new();
    collect_modules(package, &mut modules)?;
    modules.sort();
    Ok(modules)
}

/// Collect the package's own compiled modules: `.mv` files whose immediate
/// parent directory is `bytecode_modules`, which skips the framework
/// dependencies nested under `bytecode_modules/dependencies/`.
fn collect_modules(dir: &Path, modules: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_modules(&path, modules)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("mv")
            && parent_dir_name(&path) == Some("bytecode_modules")
        {
            modules.push(path);
        }
    }
    Ok(())
}

fn parent_dir_name(path: &Path) -> Option<&str> {
    path.parent()?.file_name()?.to_str()
}
