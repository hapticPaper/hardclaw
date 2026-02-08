//! HardClaw Node - Proof-of-Verification Protocol
//!
//! Run a full node that participates in the HardClaw network.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use tracing_subscriber::FmtSubscriber;

use hardclaw::{
    crypto::Keypair,
    generate_mnemonic,
    genesis::config::GenesisConfigToml,
    keypair_from_phrase,
    mempool::Mempool,
    network::{NetworkConfig, NetworkEvent, NetworkNode, PeerInfo},
    state::ChainState,
    types::{Address, Block},
    verifier::{Verifier, VerifierConfig},
};

/// Get the default data directory
fn data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".hardclaw")
}

/// Get the data directory for a specific chain ID
fn chain_data_dir(chain_id: &str) -> PathBuf {
    data_dir().join("chains").join(chain_id)
}

/// Load or generate a persistent keypair using BIP39 mnemonic
fn load_or_create_keypair() -> Keypair {
    let mnemonic_path = data_dir().join("seed_phrase.txt");
    let legacy_key_path = data_dir().join("node_key");

    // Try new format first (seed_phrase.txt)
    if mnemonic_path.exists() {
        match fs::read_to_string(&mnemonic_path) {
            Ok(phrase) => {
                let phrase = phrase.trim();
                match keypair_from_phrase(phrase, "") {
                    Ok(keypair) => {
                        info!("Loaded wallet from seed phrase at {:?}", mnemonic_path);
                        return keypair;
                    }
                    Err(e) => {
                        warn!("Invalid seed phrase file: {}", e);
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read seed phrase: {}", e);
            }
        }
    }

    // Legacy Ed25519 key files (32 bytes) are incompatible with ML-DSA-65
    if legacy_key_path.exists() {
        warn!(
            "Legacy Ed25519 key file found at {:?} — incompatible with ML-DSA-65. Generating new wallet.",
            legacy_key_path
        );
    }

    // Generate new mnemonic-based wallet
    generate_and_save_wallet(&mnemonic_path)
}

fn generate_and_save_wallet(mnemonic_path: &PathBuf) -> Keypair {
    // Ensure directory exists
    if let Some(parent) = mnemonic_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Generate new BIP39 mnemonic
    let mnemonic = generate_mnemonic();
    let phrase = mnemonic.to_string();
    let keypair = keypair_from_phrase(&phrase, "").expect("generated mnemonic is valid");

    // Save mnemonic to file with restrictive permissions
    if let Err(e) = fs::write(mnemonic_path, &phrase) {
        warn!("Failed to save seed phrase: {}", e);
    } else {
        // Set restrictive permissions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(mnemonic_path, fs::Permissions::from_mode(0o600));
        }
    }

    // Display the seed phrase prominently
    display_seed_phrase(&phrase);

    keypair
}

/// Display seed phrase with prominent warning
fn display_seed_phrase(phrase: &str) {
    let words: Vec<&str> = phrase.split_whitespace().collect();

    println!();
    println!("   ╔══════════════════════════════════════════════════════════════════════╗");
    println!("   ║                    🔐 YOUR WALLET SEED PHRASE 🔐                     ║");
    println!("   ╠══════════════════════════════════════════════════════════════════════╣");
    println!("   ║                                                                      ║");
    println!("   ║  Write down these 24 words and store them in a SAFE PLACE.          ║");
    println!("   ║  Anyone with this phrase can access your funds!                     ║");
    println!("   ║  This phrase will NOT be shown again.                               ║");
    println!("   ║                                                                      ║");
    println!("   ╠══════════════════════════════════════════════════════════════════════╣");

    // Print words in 4 columns of 6 words each
    for row in 0..6 {
        print!("   ║  ");
        for col in 0..4 {
            let idx = col * 6 + row;
            if idx < words.len() {
                print!("{:2}. {:<12} ", idx + 1, words[idx]);
            }
        }
        println!("║");
    }

    println!("   ║                                                                      ║");
    println!("   ╚══════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("   Press ENTER after you have written down your seed phrase...");

    let _ = io::stdout().flush();
    let mut input = String::new();
    let _ = io::stdin().read_line(&mut input);
}

/// Node configuration
#[derive(Clone, Debug)]
struct NodeConfig {
    /// Whether to run as a verifier
    is_verifier: bool,
    /// Network config
    network: NetworkConfig,
    /// Verifier config (if applicable)
    verifier: VerifierConfig,
    /// Listen port
    port: u16,
    /// External address for NAT traversal
    external_addr: Option<String>,
    /// Enable verbose network debug messages
    network_debug: bool,
    /// Chain ID for network isolation
    chain_id: Option<String>,
    /// Path to genesis config TOML file
    genesis_config_path: Option<PathBuf>,
    /// Reset genesis state before starting
    reset_genesis: bool,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            is_verifier: false,
            network: NetworkConfig::default(),
            verifier: VerifierConfig::default(),
            port: 9000,
            external_addr: None,
            network_debug: false,
            chain_id: None,
            genesis_config_path: None,
            reset_genesis: false,
        }
    }
}

/// The HardClaw node
struct HardClawNode {
    /// Node keypair
    keypair: Keypair,
    /// Configuration
    config: NodeConfig,
    /// Chain state
    state: Arc<RwLock<ChainState>>,
    /// Mempool
    mempool: Arc<RwLock<Mempool>>,
    /// Verifier (if running as verifier)
    verifier: Option<Verifier>,
    /// Connected peers count
    peer_count: usize,
}

impl HardClawNode {
    /// Create a new node
    fn new(keypair: Keypair, config: NodeConfig) -> Self {
        let verifier = if config.is_verifier {
            Some(Verifier::new(Keypair::generate(), config.verifier.clone()))
        } else {
            None
        };

        Self {
            keypair,
            config,
            state: Arc::new(RwLock::new(ChainState::new())),
            mempool: Arc::new(RwLock::new(Mempool::new())),
            verifier,
            peer_count: 0,
        }
    }

    /// Initialize the node
    async fn init(&mut self) -> anyhow::Result<()> {
        info!("Initializing HardClaw node...");

        // Handle genesis reset if requested
        if self.config.reset_genesis {
            if let Some(ref chain_id) = self.config.chain_id {
                if chain_id.starts_with("hardclaw-mainnet") {
                    anyhow::bail!(
                        "Refusing to reset mainnet chain '{chain_id}'. Use --force if you really mean it."
                    );
                }
                let chain_dir = chain_data_dir(chain_id);
                if chain_dir.exists() {
                    info!("Resetting genesis state for chain '{}'", chain_id);
                    fs::remove_dir_all(&chain_dir)?;
                }
            }
        }

        // Ensure chain data directory exists
        if let Some(ref chain_id) = self.config.chain_id {
            let chain_dir = chain_data_dir(chain_id);
            fs::create_dir_all(&chain_dir)?;
        }

        // Initialize genesis block if needed
        let mut state = self.state.write().await;
        if state.height() == 0 {
            // Load genesis config if provided
            if let Some(ref genesis_path) = self.config.genesis_config_path {
                info!("Loading genesis config from {:?}", genesis_path);
                let toml_config = GenesisConfigToml::load_from_file(genesis_path)?;

                // Parse pre-approved addresses (hex-encoded)
                let pre_approved: Vec<Address> = toml_config
                    .pre_approved
                    .iter()
                    .filter_map(|hex_str| {
                        let bytes = hex::decode(hex_str).ok()?;
                        if bytes.len() == 20 {
                            let mut arr = [0u8; 20];
                            arr.copy_from_slice(&bytes);
                            Some(Address::from_bytes(arr))
                        } else {
                            warn!("Skipping invalid pre-approved address (expected 20 bytes): {}", hex_str);
                            None
                        }
                    })
                    .collect();

                // Parse authority key
                let authority_bytes = hex::decode(&toml_config.authority_key)
                    .map_err(|e| anyhow::anyhow!("Invalid authority key hex: {}", e))?;
                let authority_key = hardclaw::crypto::PublicKey::from_bytes(&authority_bytes)
                    .map_err(|e| anyhow::anyhow!("Invalid authority key: {}", e))?;

                let genesis_config = hardclaw::genesis::GenesisConfig::new(
                    toml_config.chain_id.clone(),
                    pre_approved,
                    authority_key,
                    0,
                );

                info!(
                    chain_id = %genesis_config.chain_id,
                    "Creating genesis block with config"
                );
                let genesis = Block::genesis_with_config(
                    self.keypair.public_key().clone(),
                    genesis_config,
                );
                state.apply_block(genesis)?;
            } else {
                info!("Creating genesis block (no genesis config)...");
                let genesis = Block::genesis(self.keypair.public_key().clone());
                state.apply_block(genesis)?;
            }
        }

        info!("Node initialized at height {}", state.height());
        Ok(())
    }

    /// Run the node
    async fn run(&mut self) -> anyhow::Result<()> {
        info!("Starting HardClaw node...");

        // Configure network
        let mut network_config = self.config.network.clone();
        network_config.listen_addr = format!("/ip4/0.0.0.0/tcp/{}", self.config.port);
        network_config.external_addr = self.config.external_addr.clone();

        // Create peer info
        let peer_info = PeerInfo {
            public_key: self.keypair.public_key().clone(),
            kem_public_key: None,
            address: network_config.listen_addr.clone(),
            is_verifier: self.config.is_verifier,
            version: 1,
            chain_id: self.config.chain_id.clone(),
        };

        // Create network node
        let (mut network, mut event_rx) = NetworkNode::new(network_config, peer_info)?;

        let peer_id = network.local_peer_id();

        // Start network
        network.start().await?;

        if self.verifier.is_some() {
            info!("Running as verifier");
        } else {
            info!("Running as full node");
        }

        if self.config.network_debug {
            info!(
                "Connect to this node: /ip4/<IP>/tcp/{}/p2p/{}",
                self.config.port, peer_id
            );
        }

        // Main event loop - drive the swarm and handle application events
        let is_verifier = self.verifier.is_some();
        loop {
            tokio::select! {
                // Drive the libp2p swarm (processes dials, DNS, connections)
                _ = network.poll() => {}

                // Handle network events forwarded from the swarm
                Some(event) = event_rx.recv() => {
                    self.handle_network_event(event).await;
                }

                // Node tick (process verifier/node logic)
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                    if is_verifier {
                        self.process_verifier_tick().await?;
                    }
                }
            }
        }
    }

    /// Handle network events
    async fn handle_network_event(&mut self, event: NetworkEvent) {
        let debug = self.config.network_debug;

        match event {
            NetworkEvent::PeerConnected(peer) => {
                self.peer_count += 1;
                info!(
                    "Connected to {} peer{}",
                    self.peer_count,
                    if self.peer_count == 1 { "" } else { "s" }
                );
                if debug {
                    info!("Peer connected: {}", peer);
                }
            }
            NetworkEvent::PeerDisconnected(peer) => {
                if self.peer_count > 0 {
                    self.peer_count -= 1;
                }
                info!(
                    "Connected to {} peer{}",
                    self.peer_count,
                    if self.peer_count == 1 { "" } else { "s" }
                );
                if debug {
                    info!("Peer disconnected: {}", peer);
                }
                if self.peer_count == 0 {
                    info!("Waiting for peer connections...");
                }
            }
            NetworkEvent::JobReceived(job) => {
                if debug {
                    info!("Received job: {}", job.id);
                }
                let mut mp = self.mempool.write().await;
                if let Err(e) = mp.add_job(*job) {
                    warn!("Failed to add job to mempool: {}", e);
                }
            }
            NetworkEvent::SolutionReceived(solution) => {
                if debug {
                    info!("Received solution: {}", solution.id);
                }
            }
            NetworkEvent::BlockReceived(block) => {
                info!("Received block at height {}", block.header.height);
                if debug {
                    info!("Block hash: {}", block.hash);
                }
                let mut st = self.state.write().await;
                if let Err(e) = st.apply_block(*block) {
                    warn!("Failed to apply block: {}", e);
                }
            }
            NetworkEvent::AttestationReceived(attestation) => {
                if debug {
                    info!("Received attestation for block {}", attestation.block_hash);
                }
            }
            NetworkEvent::PeersDiscovered(peers) => {
                if debug {
                    info!("Discovered {} peers via DHT", peers.len());
                }
            }
            NetworkEvent::Started {
                peer_id,
                listen_addr,
            } => {
                info!("Network started");
                info!("P2P Peer ID: {}", peer_id);
                if debug {
                    info!("Listen address: {}", listen_addr);
                }
            }
            NetworkEvent::Error(e) => {
                warn!("Network error: {}", e);
            }
        }
    }

    /// Process one verifier tick
    async fn process_verifier_tick(&mut self) -> anyhow::Result<()> {
        let verifier = self.verifier.as_mut().expect("verifier mode");
        // Process pending solutions from mempool
        let solutions = {
            let mut mempool = self.mempool.write().await;
            mempool.pop_solutions(100)
        };

        for (job, solution) in solutions {
            match verifier.process_solution(&job, &solution) {
                Ok((result, is_honey_pot)) => {
                    if result.passed {
                        info!("Solution {} verified for job {}", solution.id, job.id);
                    } else {
                        info!("Solution {} rejected for job {}", solution.id, job.id);
                    }
                    if is_honey_pot {
                        info!("Honey pot detected!");
                    }
                }
                Err(e) => {
                    warn!("Verification error: {}", e);
                }
            }
        }

        // Try to produce a block
        let state_root = self.state.read().await.compute_state_root();
        if let Some(block) = verifier.try_produce_block(state_root)? {
            info!(
                "Produced block {} at height {}",
                block.hash, block.header.height
            );
            let mut state = self.state.write().await;
            state.apply_block(block)?;
        }

        Ok(())
    }
}

/// Special CLI commands that exit immediately
enum NodeCommand {
    Run(NodeConfig),
    ShowSeed,
    Recover,
}

fn parse_args(args: Vec<String>) -> NodeCommand {
    let mut config = NodeConfig::default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--show-seed" => return NodeCommand::ShowSeed,
            "--recover" => return NodeCommand::Recover,
            "--verifier" | "-v" => config.is_verifier = true,
            "--network-debug" => config.network_debug = true,
            "--port" | "-p" => {
                i += 1;
                if i < args.len() {
                    config.port = args[i].parse().unwrap_or(9000);
                }
            }
            "--bootstrap" | "-b" => {
                i += 1;
                if i < args.len() {
                    config.network.bootstrap_peers.push(args[i].clone());
                }
            }
            "--external-addr" => {
                i += 1;
                if i < args.len() {
                    config.external_addr = Some(args[i].clone());
                }
            }
            "--no-official-bootstrap" => {
                config.network.use_official_bootstrap = false;
            }
            "--chain-id" => {
                i += 1;
                if i < args.len() {
                    config.chain_id = Some(args[i].clone());
                    config.network.chain_id = Some(args[i].clone());
                }
            }
            "--genesis" => {
                i += 1;
                if i < args.len() {
                    config.genesis_config_path = Some(PathBuf::from(&args[i]));
                }
            }
            "--reset-genesis" => {
                config.reset_genesis = true;
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }

    NodeCommand::Run(config)
}

fn print_help() {
    println!("HardClaw Node");
    println!();
    println!("USAGE:");
    println!("    hardclaw node [OPTIONS]");
    println!();
    println!("WALLET COMMANDS:");
    println!("    --show-seed                 Display your wallet seed phrase");
    println!("    --recover                   Recover wallet from seed phrase");
    println!();
    println!("NODE OPTIONS:");
    println!("    -v, --verifier              Run as a verifier node");
    println!("    -p, --port <PORT>           Listen port (default: 9000)");
    println!("    -b, --bootstrap <ADDR>      Bootstrap peer address");
    println!("    --external-addr <ADDR>      External address for NAT traversal");
    println!("    --network-debug             Enable verbose network logging");
    println!("    --no-official-bootstrap     Don't use official bootstrap nodes");
    println!("    --chain-id <ID>             Chain ID for network isolation");
    println!("    --genesis <PATH>            Path to genesis config TOML file");
    println!("    --reset-genesis             Wipe chain state and re-init from genesis");
    println!("    -h, --help                  Print help");
}

/// Show the current wallet's seed phrase
fn show_seed() {
    let mnemonic_path = data_dir().join("seed_phrase.txt");

    if !mnemonic_path.exists() {
        println!("No wallet found. Run the node first to create a wallet.");
        std::process::exit(1);
    }

    match fs::read_to_string(&mnemonic_path) {
        Ok(phrase) => {
            println!();
            println!("Your wallet seed phrase (keep this secret!):");
            println!();
            let words: Vec<&str> = phrase.split_whitespace().collect();
            for (i, word) in words.iter().enumerate() {
                print!("{:2}. {:<12} ", i + 1, word);
                if (i + 1) % 4 == 0 {
                    println!();
                }
            }
            println!();
        }
        Err(e) => {
            println!("Failed to read seed phrase: {}", e);
            std::process::exit(1);
        }
    }
}

/// Recover wallet from seed phrase
fn recover_wallet() {
    let mnemonic_path = data_dir().join("seed_phrase.txt");

    if mnemonic_path.exists() {
        println!("A wallet already exists at {:?}", mnemonic_path);
        println!("To recover, first backup and delete the existing seed_phrase.txt");
        std::process::exit(1);
    }

    println!("Enter your 24-word seed phrase (space-separated):");
    print!("> ");
    let _ = io::stdout().flush();

    let mut phrase = String::new();
    if io::stdin().read_line(&mut phrase).is_err() {
        println!("Failed to read input");
        std::process::exit(1);
    }

    let phrase = phrase.trim();
    let word_count = phrase.split_whitespace().count();
    if word_count != 24 {
        println!("Expected 24 words, got {}", word_count);
        std::process::exit(1);
    }

    // Validate the mnemonic
    match keypair_from_phrase(phrase, "") {
        Ok(keypair) => {
            // Save the mnemonic
            if let Some(parent) = mnemonic_path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            if let Err(e) = fs::write(&mnemonic_path, phrase) {
                println!("Failed to save seed phrase: {}", e);
                std::process::exit(1);
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&mnemonic_path, fs::Permissions::from_mode(0o600));
            }

            let address = Address::from_public_key(keypair.public_key());
            println!();
            println!("Wallet recovered successfully!");
            println!("Address: {}", address);
            println!("Saved to: {:?}", mnemonic_path);
        }
        Err(e) => {
            println!("Invalid seed phrase: {}", e);
            std::process::exit(1);
        }
    }
}

#[tokio::main]
pub async fn run(args: Vec<String>) -> anyhow::Result<()> {
    // Parse CLI first (before logging, since some commands are interactive)
    let command = parse_args(args);

    // Handle wallet commands (non-node operations)
    match &command {
        NodeCommand::ShowSeed => {
            show_seed();
            return Ok(());
        }
        NodeCommand::Recover => {
            recover_wallet();
            return Ok(());
        }
        NodeCommand::Run(_) => {}
    }

    let config = match command {
        NodeCommand::Run(c) => c,
        _ => unreachable!(),
    };

    // Initialize logging with EnvFilter to support RUST_LOG
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(env_filter)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    println!();
    println!("   ██╗  ██╗ █████╗ ██████╗ ██████╗  ██████╗██╗      █████╗ ██╗    ██╗");
    println!("   ██║  ██║██╔══██╗██╔══██╗██╔══██╗██╔════╝██║     ██╔══██╗██║    ██║");
    println!("   ███████║███████║██████╔╝██║  ██║██║     ██║     ███████║██║ █╗ ██║");
    println!("   ██╔══██║██╔══██║██╔══██╗██║  ██║██║     ██║     ██╔══██║██║███╗██║");
    println!("   ██║  ██║██║  ██║██║  ██║██████╔╝╚██████╗███████╗██║  ██║╚███╔███╔╝");
    println!("   ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝╚═════╝  ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝");
    println!();
    println!("   Proof-of-Verification Protocol v{}", hardclaw::VERSION);
    println!("   \"We do not trust; we verify.\"");
    println!();

    // Load or generate persistent keypair
    let keypair = load_or_create_keypair();
    let address = Address::from_public_key(keypair.public_key());

    info!("Node address: {}", address);

    // Create and run node
    let mut node = HardClawNode::new(keypair, config);
    node.init().await?;
    node.run().await?;

    Ok(())
}
