use clap::{Parser, Subcommand};

mod check;
mod locale;
mod source_scan;
mod sync;

#[derive(Parser)]
#[command(name = "xtask", about = "Repository maintenance tasks")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check for missing, unused, or hardcoded translation strings (CI gate)
    CheckKeys,
    /// Scaffold missing translation keys as empty stubs in all locale files
    SyncKeys,
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::CheckKeys => check::run(),
        Command::SyncKeys => sync::run(),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
