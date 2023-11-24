use clap::Args;

#[derive(Args)]
pub(crate) struct ControllerArgs {
    /// Use this flag if you want to let shoebill
    /// update secrets that already exist in the cluster
    #[arg(long, default_value_t = false, env = "SHOEBILL_ALLOW_EXISTING")]
    pub(crate) allow_existing: bool,
}
