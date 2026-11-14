//! The `fulgurstack` command line. Each command lives in its own file under
//! `cmd/`; the options several commands share live in `calibration`, `lights`
//! and `sky`, and report destinations in `files`.
mod calibration;
mod cmd;
mod files;
mod lights;
mod sky;

use clap::{Parser, Subcommand};
use fulgur_core::{Error, Result};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set by Ctrl+C; every long step checks it.
pub(crate) static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Parser)]
#[command(
    name = "fulgurstack",
    version,
    about = "Linear FITS and XISF processing: reconstruction, quality inspection and stacking."
)]
struct Cli {
    /// CPU pool threads (default: available parallelism, capped at 24; GPU at 16).
    #[arg(long, global = true, value_parser = clap::value_parser!(u16).range(1..=256))]
    threads: Option<u16>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect, register and integrate immutable CFA lights into a linear master.
    Stack(cmd::stack::StackArgs),
    /// Register CFA catalogs to an automatic anchor and plan output canvases; no image files.
    Register(cmd::register::RegisterArgs),
    /// Inspect source CFA quality and produce a private JSON catalog and review table.
    Inspect(cmd::inspect::InspectArgs),
    /// Combine dark frames into a master dark FITS (32-bit float ADU) for later runs.
    MasterDark(cmd::master_dark::MasterDarkArgs),
    /// Reconstruct one CFA FITS into a linear float RGB FITS without intermediate images.
    DebayerSingle(cmd::debayer_single::DebayerSingleArgs),
    /// Survey FITS headers and measure full-file read throughput without writing images.
    Survey(cmd::survey::SurveyArgs),
    /// Check a local Gaia catalogue installation (PixInsight XPSD files):
    /// readable, consistent and without gaps in its magnitude coverage.
    GaiaCheck(cmd::gaia_check::GaiaCheckArgs),
    /// Place frames on the sky with no position given (blind plate solving
    /// against Gaia DR3, with the index of `solve-index build`).
    Solve(cmd::solve::SolveArgs),
    /// Build or describe the blind plate solving index of Gaia DR3 stars.
    SolveIndex(cmd::solve::IndexArgs),
}

fn run(cli: Cli) -> Result<()> {
    // GPU reconstruction dispatches at most 16 ROIs; CPU reconstruction uses 24.
    let default_limit = match &cli.command {
        Command::Stack(args) if args.gpu_reconstruction() => 16,
        _ => 24,
    };
    let threads = cli.threads.map(usize::from).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1)
            .min(default_limit)
    });
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build_global()
        .map_err(|_| Error::Invalid("Could not initialize CPU workers."))?;
    ctrlc::set_handler(|| CANCEL.store(true, Ordering::Relaxed))
        .map_err(|_| Error::Invalid("Could not install cancellation handler."))?;
    match cli.command {
        Command::Stack(args) => cmd::stack::run(args),
        Command::Register(args) => cmd::register::run(args),
        Command::Inspect(args) => cmd::inspect::run(args),
        Command::MasterDark(args) => cmd::master_dark::run(args),
        Command::DebayerSingle(args) => cmd::debayer_single::run(args, threads),
        Command::Survey(args) => cmd::survey::run(args),
        Command::GaiaCheck(args) => cmd::gaia_check::run(args),
        Command::Solve(args) => cmd::solve::run(args),
        Command::SolveIndex(args) => cmd::solve::run_index(args),
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FAIL: {error}");
            if matches!(error, Error::Cancelled) {
                ExitCode::from(130)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
