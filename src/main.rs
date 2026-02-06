//! HardClaw - Proof-of-Verification Protocol
//!
//! Single binary with subcommands:
//!   hardclaw           - TUI onboarding (default)
//!   hardclaw node      - Run a full node / verifier
//!   hardclaw cli       - Interactive CLI

mod cli;
mod node;
mod onboarding;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("node") => {
            // Pass remaining args (skip binary name and "node" subcommand)
            let node_args = args[2..].to_vec();
            if let Err(e) = node::run(node_args) {
                eprintln!("Node error: {}", e);
                std::process::exit(1);
            }
        }
        Some("cli") => {
            cli::run();
        }
        Some("--help") | Some("-h") => {
            print_help();
        }
        Some("--version") | Some("-V") => {
            println!("hardclaw {}", hardclaw::VERSION);
        }
        _ => {
            // Default: launch TUI onboarding
            if let Err(e) = onboarding::run() {
                eprintln!("TUI error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn print_help() {
    println!("HardClaw v{}", hardclaw::VERSION);
    println!("Proof-of-Verification Protocol");
    println!();
    println!("USAGE:");
    println!("    hardclaw [COMMAND]");
    println!();
    println!("COMMANDS:");
    println!("    (default)   Launch the onboarding TUI");
    println!("    node        Run a full node or verifier");
    println!("    cli         Interactive CLI for wallet & jobs");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help      Print help");
    println!("    -V, --version   Print version");
    println!();
    println!("EXAMPLES:");
    println!("    hardclaw                     Start the TUI");
    println!("    hardclaw node --verifier     Run as verifier node");
    println!("    hardclaw node --help         Show node options");
    println!("    hardclaw cli                 Start interactive CLI");
}
