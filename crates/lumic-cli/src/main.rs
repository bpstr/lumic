use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "lumic", version, about = "Host-native Linux server management")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Print Lumic version.
    Version,
    /// Inspect basic host status.
    Status,
}

fn main() {
    match Cli::parse().command.unwrap_or(Command::Status) {
        Command::Version => println!("lumic {}", env!("CARGO_PKG_VERSION")),
        Command::Status => {
            let facts = lumic_platform::inspect_host();
            println!("Lumic {}", env!("CARGO_PKG_VERSION"));
            println!("OS: {}", facts.os);
            println!("Architecture: {}", facts.architecture);
        }
    }
}
