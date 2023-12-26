use clap::{Args, Command, Parser, Subcommand};

#[derive(Args)]
pub(crate) struct ManifestsArgs {
    #[arg(long, short, default_value = "default")]
    pub(crate) namespace: String,
    #[arg(long, short, default_value = "latest")]
    pub(crate) tag: String,
    #[arg(long, short, default_value = "shoebill")]
    pub(crate) image: String,
}
