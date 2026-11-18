//! `fabrial run`: the chain live, as a virtual mic named "fabrial".

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use fabrial_core::ChainFile;

#[derive(clap::Args)]
pub struct Args {
    /// The chain file.
    #[arg(short, long)]
    chain: PathBuf,
    /// The mic to read, by PipeWire node name. Default: the system's default
    /// mic.
    #[arg(long)]
    mic: Option<String>,
    /// The least time the chain's thread gets for each block, in ms. It adds
    /// to the delay.
    #[arg(long, default_value_t = 10.)]
    margin: f32,
    /// Seconds between stats lines; 0 to print one only when something goes
    /// wrong (gaps, lost audio, the mic going silent).
    #[arg(long, default_value_t = 10)]
    stats: u64,
    /// Leave apps where they are. By default, apps that record from the mic
    /// are moved onto the "fabrial" mic.
    #[arg(long)]
    no_move: bool,
}

pub fn run(a: Args) -> Result<()> {
    fabrial_pipewire::run(fabrial_pipewire::Options {
        chain: ChainFile::load(&a.chain)?,
        mic: a.mic,
        margin_ms: a.margin,
        stats_every: (a.stats > 0).then(|| Duration::from_secs(a.stats)),
        move_apps: !a.no_move,
    })
}
