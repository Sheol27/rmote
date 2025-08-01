use clap::{ArgAction, Parser};

/// Simple, fast SFTP directory mirror: local -> remote
#[derive(Parser, Debug)]
#[command(name = "rmote", author, version, about)]
pub struct Cli {
    /// Remote host (IP or DNS)
    #[arg(long, env = "RMOTE_HOST")]
    pub host: String,

    /// Remote SSH port
    #[arg(long, env = "RMOTE_PORT", default_value = "22")]
    pub port: u16,

    /// SSH username
    #[arg(long, env = "RMOTE_USER", default_value = "root")]
    pub user: String,

    /// Path to private key (e.g. ~/.ssh/id_ed25519)
    #[arg(long, env = "RMOTE_KEY", default_value = "~/.ssh/id_ed25519")]
    pub identity: String,

    /// Path to public key (e.g. ~/.ssh/id_ed25519.pub)
    #[arg(long, env = "RMOTE_PUB", default_value = "~/.ssh/id_ed25519.pub")]
    pub identity_pub: String,

    /// Optional passphrase for the private key
    #[arg(long, env = "RMOTE_PASSPHRASE")]
    pub passphrase: Option<String>,

    /// Remote base directory to mirror into (created if needed)
    #[arg(long, env = "RMOTE_REMOTE_DIR", default_value = ".")]
    pub remote_dir: String,

    /// Perform a full sync at startup
    #[arg(long, action = ArgAction::SetTrue, default_value_t = true)]
    pub initial_sync: bool,

    /// Disable full sync at startup
    #[arg(long, action = ArgAction::SetTrue, overrides_with = "initial_sync")]
    pub no_initial_sync: bool,

    /// One or more blacklist entries. May be repeated.
    /// Matches if a path equals an entry or starts with it.
    #[arg(long = "blacklist", short = 'x', action = ArgAction::Append)]
    pub blacklist: Vec<String>,

    /// Debounce window (seconds) to coalesce events
    #[arg(long, default_value_t = 1)]
    pub debounce_s: u64,
}
