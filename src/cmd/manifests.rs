use clap::{Args, Command, Parser, Subcommand};

#[derive(Args)]
pub(crate) struct ManifestsArgs {
    #[arg(long, short, default_value = "default")]
    pub(crate) namespace: String,
}
