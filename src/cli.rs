//! HardClaw CLI - Command line interface for the HardClaw protocol

use std::io::{self, Write};

use hardclaw::{
    crypto::{Keypair, hash_data},
    generate_mnemonic, keypair_from_mnemonic,
    types::{Address, JobPacket, JobType, HclawAmount, VerificationSpec},
};
use sha2::{Sha256, Digest};

fn main() {
    println!("╔════════════════════════════════════════════╗");
    println!("║       HardClaw CLI v{}             ║", hardclaw::VERSION);
    println!("║   Proof-of-Verification Protocol          ║");
    println!("╚════════════════════════════════════════════╝");
    println!();

    // Generate a keypair for this session
    let keypair = Keypair::generate();
    let address = Address::from_public_key(keypair.public_key());

    println!("Session address: {}", address);
    println!();
    println!("Commands:");
    println!("  wallet [count]  - Generate wallet(s) with seed phrases");
    println!("  keygen          - Generate a new keypair (no seed phrase)");
    println!("  balance <addr>  - Check account balance");
    println!("  submit <job>    - Submit a job");
    println!("  status <id>     - Check job status");
    println!("  verify <id>     - Verify a solution");
    println!("  help            - Show this help");
    println!("  quit            - Exit");
    println!();

    loop {
        print!("hclaw> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            break;
        }

        let parts: Vec<&str> = input.trim().split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "wallet" => {
                let count: usize = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                println!();
                println!("╔════════════════════════════════════════════════════════════════════════╗");
                println!("║  IMPORTANT: Write down these seed phrases and store them SECURELY!    ║");
                println!("╚════════════════════════════════════════════════════════════════════════╝");
                println!();

                for i in 1..=count {
                    let mnemonic = generate_mnemonic();
                    let phrase = mnemonic.to_string();
                    let new_keypair = keypair_from_mnemonic(&mnemonic, "");
                    let new_address = Address::from_public_key(new_keypair.public_key());

                    // Derive libp2p peer ID for bootstrap node setup
                    let mut hasher = Sha256::new();
                    hasher.update(b"hardclaw-libp2p-identity-v1");
                    hasher.update(new_keypair.public_key().as_bytes());
                    let hash = hasher.finalize();
                    let mut hash_bytes: [u8; 32] = hash.into();
                    let secret = libp2p::identity::ed25519::SecretKey::try_from_bytes(&mut hash_bytes)
                        .expect("SHA-256 output is valid Ed25519 seed");
                    let ed25519_kp = libp2p::identity::ed25519::Keypair::from(secret);
                    let libp2p_kp = libp2p::identity::Keypair::from(ed25519_kp);
                    let peer_id = libp2p_kp.public().to_peer_id();

                    println!("═══════════════════════════════════════════════════════════════════════");
                    println!("  WALLET {}", i);
                    println!("═══════════════════════════════════════════════════════════════════════");
                    println!("  Address:  {}", new_address);
                    println!("  Peer ID:  {}", peer_id);
                    println!();
                    println!("  Seed Phrase (24 words):");
                    let words: Vec<&str> = phrase.split_whitespace().collect();
                    for row in 0..6 {
                        print!("    ");
                        for col in 0..4 {
                            let idx = col * 6 + row;
                            print!("{:2}. {:<12} ", idx + 1, words[idx]);
                        }
                        println!();
                    }
                    println!();
                    println!("  Copy-paste format:");
                    println!("  {}", phrase);
                    println!();
                }
                println!("═══════════════════════════════════════════════════════════════════════");
            }

            "keygen" => {
                let new_keypair = Keypair::generate();
                let new_address = Address::from_public_key(new_keypair.public_key());
                println!("Generated new keypair:");
                println!("  Address: {}", new_address);
                println!("  Public Key: {}", new_keypair.public_key().to_hex());
            }

            "balance" => {
                if parts.len() < 2 {
                    println!("Usage: balance <address>");
                    continue;
                }
                // In a full implementation, this would query the node
                println!("Balance for {}: 0.0 HCLAW (not connected to network)", parts[1]);
            }

            "submit" => {
                println!("Creating a new job...");
                println!();

                // Interactive job creation
                print!("Job description: ");
                io::stdout().flush().unwrap();
                let mut description = String::new();
                io::stdin().read_line(&mut description).unwrap();

                print!("Bounty (HCLAW): ");
                io::stdout().flush().unwrap();
                let mut bounty_str = String::new();
                io::stdin().read_line(&mut bounty_str).unwrap();
                let bounty: u64 = bounty_str.trim().parse().unwrap_or(10);

                print!("Expected output hash (or 'none' for subjective): ");
                io::stdout().flush().unwrap();
                let mut hash_str = String::new();
                io::stdin().read_line(&mut hash_str).unwrap();

                let (job_type, verification) = if hash_str.trim() == "none" {
                    (
                        JobType::Subjective,
                        VerificationSpec::SchellingPoint {
                            min_voters: 3,
                            quality_threshold: 70,
                        }
                    )
                } else {
                    let expected_hash = if hash_str.trim().is_empty() {
                        hash_data(b"placeholder")
                    } else {
                        hardclaw::crypto::Hash::from_hex(hash_str.trim()).unwrap_or_else(|_| hash_data(b"placeholder"))
                    };
                    (
                        JobType::Deterministic,
                        VerificationSpec::HashMatch { expected_hash }
                    )
                };

                let job = JobPacket::new(
                    job_type,
                    *keypair.public_key(),
                    b"input data".to_vec(),
                    description.trim().to_string(),
                    HclawAmount::from_hclaw(bounty),
                    HclawAmount::from_hclaw(1), // Burn fee
                    verification,
                    3600,
                );

                println!();
                println!("Job created:");
                println!("  ID: {}", job.id);
                println!("  Type: {:?}", job.job_type);
                println!("  Bounty: {} HCLAW", bounty);
                println!("  Burn Fee: 1 HCLAW");
                println!("  Expires: {} seconds", 3600);
                println!();
                println!("(In a connected network, this would be broadcast to the mempool)");
            }

            "status" => {
                if parts.len() < 2 {
                    println!("Usage: status <job_id>");
                    continue;
                }
                println!("Job {} status: Unknown (not connected to network)", parts[1]);
            }

            "verify" => {
                if parts.len() < 2 {
                    println!("Usage: verify <solution_id>");
                    continue;
                }
                println!("Solution {} verification: Not implemented in CLI mode", parts[1]);
            }

            "help" => {
                println!("Commands:");
                println!("  wallet [count]  - Generate wallet(s) with seed phrases");
                println!("  keygen          - Generate a new keypair (no seed phrase)");
                println!("  balance <addr>  - Check account balance");
                println!("  submit          - Submit a job interactively");
                println!("  status <id>     - Check job status");
                println!("  verify <id>     - Verify a solution");
                println!("  help            - Show this help");
                println!("  quit            - Exit");
            }

            "quit" | "exit" | "q" => {
                println!("Goodbye!");
                break;
            }

            _ => {
                println!("Unknown command: {}. Type 'help' for available commands.", parts[0]);
            }
        }

        println!();
    }
}
