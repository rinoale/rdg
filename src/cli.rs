use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "rdg",
    version,
    about = "Remote deploy selected git changes via rsync"
)]
pub struct Cli {
    /// Rsync destination, for example:
    /// user@example.com:/var/www/my-app/
    ///
    /// You can also set RDG_TARGET.
    #[arg(value_name = "TARGET", env = "RDG_TARGET")]
    pub target: Option<String>,
}
