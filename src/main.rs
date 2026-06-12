mod app;
mod cli;
mod core;
mod tui;
mod ui;

use anyhow::{Context, Result};
use clap::Parser;

use crate::{app::App, cli::Cli};

fn main() -> Result<()> {
    let cli = Cli::parse();

    let target = cli.target.context(
        "Missing target.\n\nUsage:\n  rdg user@example.com:/var/www/my-app/\n\nOr:\n  RDG_TARGET=user@example.com:/var/www/my-app/ rdg",
    )?;

    tui::run(App::new(target)?)
}
