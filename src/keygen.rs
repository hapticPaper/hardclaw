//! Key generation utility for HardClaw
//!
//! Generates ML-DSA-65 keypairs for wallets and authority keys.
//!
//! Usage:
//!   hardclaw keygen                  Generate a new wallet
//!   hardclaw keygen --seed           Derive wallet from existing seed phrase
//!   hardclaw keygen --authority      Generate authority keypair (requires --seed)

use hardclaw::wallet::Wallet;

pub fn run(args: &[String]) {
    let has_seed = args.iter().any(|a| a == "--seed");
    let has_authority = args.iter().any(|a| a == "--authority");

    if has_authority {
        run_authority(has_seed);
    } else if has_seed {
        run_from_seed();
    } else {
        run_generate();
    }
}

/// Generate a fresh wallet with a new mnemonic
fn run_generate() {
    println!("Generating new HardClaw wallet...");
    println!("----------------------------------------------------------------");

    let mut wallet = Wallet::generate();
    let address = wallet.address();
    let phrase = wallet
        .mnemonic
        .as_ref()
        .expect("generated wallet has mnemonic");

    println!("Seed Phrase (KEEP THIS SAFE — loss = loss of funds):");
    println!("{}", phrase);
    println!();
    println!("Public Key (Hex):");
    println!("{}", wallet.public_key().to_hex());
    println!();
    println!("Address:");
    println!("{}", address);
    println!("----------------------------------------------------------------");

    save_wallet(&mut wallet);
}

/// Derive wallet from an existing seed phrase
fn run_from_seed() {
    println!("Restore wallet from seed phrase");
    println!("----------------------------------------------------------------");
    println!("Enter your 24-word seed phrase:");

    let mut phrase = String::new();
    std::io::stdin()
        .read_line(&mut phrase)
        .expect("failed to read input");
    let phrase = phrase.trim();

    if phrase.is_empty() {
        eprintln!("Error: empty seed phrase");
        std::process::exit(1);
    }

    let keypair = match hardclaw::keypair_from_phrase(phrase, "") {
        Ok(kp) => kp,
        Err(e) => {
            eprintln!("Error: invalid seed phrase: {}", e);
            std::process::exit(1);
        }
    };

    let mut wallet = Wallet::from_keypair_and_mnemonic(keypair, phrase.to_string());
    let address = wallet.address();

    println!();
    println!("Public Key (Hex):");
    println!("{}", wallet.public_key().to_hex());
    println!();
    println!("Address:");
    println!("{}", address);
    println!("----------------------------------------------------------------");

    save_wallet(&mut wallet);
}

/// Generate an authority keypair (for signing genesis config).
/// Authority keys MUST be derived from a seed for recoverability.
fn run_authority(has_seed: bool) {
    if !has_seed {
        eprintln!("Error: --authority requires --seed for recoverability");
        eprintln!("Usage: hardclaw keygen --authority --seed");
        std::process::exit(1);
    }

    println!("Generate authority keypair from seed phrase");
    println!("----------------------------------------------------------------");
    println!("Enter your 24-word seed phrase for the authority key:");

    let mut phrase = String::new();
    std::io::stdin()
        .read_line(&mut phrase)
        .expect("failed to read input");
    let phrase = phrase.trim();

    if phrase.is_empty() {
        eprintln!("Error: empty seed phrase");
        std::process::exit(1);
    }

    let keypair = match hardclaw::keypair_from_phrase(phrase, "") {
        Ok(kp) => kp,
        Err(e) => {
            eprintln!("Error: invalid seed phrase: {}", e);
            std::process::exit(1);
        }
    };

    let mut wallet = Wallet::from_keypair_and_mnemonic(keypair, phrase.to_string());
    wallet.name = Some("authority".to_string());
    let address = wallet.address();

    println!();
    println!("Authority Public Key (Hex) — put this in hardclaw.toml authority_key:");
    println!("{}", wallet.public_key().to_hex());
    println!();
    println!("Authority Address:");
    println!("{}", address);
    println!("----------------------------------------------------------------");

    // Save as authority.json in wallets dir
    let wallets_dir = Wallet::default_dir();
    let authority_path = wallets_dir.join("authority.json");
    let addr_path = wallets_dir.join(format!("{}.json", address));

    match wallet.save(&addr_path) {
        Ok(()) => {
            println!("Wallet saved to: {}", addr_path.display());
            // Also save a copy as authority.json for easy reference
            if let Err(e) = wallet.save(&authority_path) {
                eprintln!("Warning: failed to save authority.json copy: {}", e);
            } else {
                println!("Authority copy:  {}", authority_path.display());
            }
        }
        Err(e) => {
            eprintln!("Failed to save wallet: {}", e);
            std::process::exit(1);
        }
    }
}

/// Save wallet as <address>.json and set as default if none exists
fn save_wallet(wallet: &mut Wallet) {
    let address = wallet.address();
    let wallets_dir = Wallet::default_dir();
    let path = wallets_dir.join(format!("{}.json", address));

    match wallet.save(&path) {
        Ok(()) => {
            println!("Wallet saved to: {}", path.display());

            if !Wallet::default_exists() {
                if let Err(e) = wallet.save_as_default() {
                    eprintln!("Warning: failed to set as default wallet: {}", e);
                } else {
                    println!("Set as default wallet");
                }
            } else {
                println!("Default wallet already exists. Use TUI to switch.");
            }
        }
        Err(e) => {
            eprintln!("Failed to save wallet: {}", e);
            std::process::exit(1);
        }
    }
}
