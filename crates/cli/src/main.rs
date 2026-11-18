mod audio;
mod render;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about = "Mic enhancer for calls")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs a recorded take through a chain, as the live mic would have.
    Render(render::Args),
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Render(a) => render::run(a),
    }
}

/// Parses a time as seconds, m:ss or h:mm:ss (decimals allowed in the
/// seconds).
pub fn parse_time(s: &str) -> Result<f64, String> {
    let mut total = 0.;
    for part in s.split(':') {
        let v: f64 = part.parse().map_err(|_| format!("not a time: {s}"))?;
        total = total * 60. + v;
    }
    Ok(total)
}

/// Where a take keeps its renders.
pub fn renders_dir(take: &std::path::Path) -> PathBuf {
    take.join("renders")
}
