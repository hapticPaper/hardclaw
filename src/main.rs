//! HardClaw - Proof-of-Verification Protocol
//!
//! Single binary with subcommands:
//!   hardclaw           - TUI onboarding (default)
//!   hardclaw node      - Run a full node / verifier
//!   hardclaw cli       - Interactive CLI

mod cli;
mod keygen;
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
        Some("keygen") => {
            let keygen_args = args[2..].to_vec();
            keygen::run(&keygen_args);
        }
        Some("--help") | Some("-h") => {
            print_help();
        }
        Some("--version") | Some("-V") => {
            println!("hardclaw {}", hardclaw::VERSION);
        }
        _ => {
            // Default: launch TUI onboarding
            match onboarding::run() {
                Ok(true) => {
                    // User selected "Run Verifier Node" - start it
                    println!("Starting verifier node...\n");
                    if let Err(e) = node::run(vec!["--verifier".to_string()]) {
                        eprintln!("Node error: {}", e);
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("TUI error: {}", e);
                    std::process::exit(1);
                }
                Ok(false) => {
                    // Normal TUI exit
                }
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
    println!("    keygen      Generate a new wallet/keypair");
    println!("                  --seed       Derive from existing seed phrase");
    println!("                  --authority  Generate authority key (requires --seed)");
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
    println!("    hardclaw keygen              Generate keys");
}
