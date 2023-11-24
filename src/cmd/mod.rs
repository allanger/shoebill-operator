use clap::{command, Parser, Subcommand};

use self::controller::ControllerArgs;
use self::manifests::ManifestsArgs;

pub(crate) mod controller;
pub(crate) mod manifests;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    // Start the controller
    Controller(ControllerArgs),
    // Generate manifests for quick install
    Manifests(ManifestsArgs),
}
