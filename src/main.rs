//! HardClaw Node - Proof-of-Verification Protocol
//!
//! Run a full node that participates in the HardClaw network.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, Level};
use tracing_subscriber::FmtSubscriber;

use hardclaw::{
    crypto::Keypair,
    types::{Address, Block},
    verifier::{Verifier, VerifierConfig},
    tokenomics::TokenEconomics,
    mempool::Mempool,
    state::ChainState,
    network::{NetworkConfig, NetworkNode, NetworkEvent, PeerInfo},
};

/// Node configuration
#[derive(Clone, Debug)]
struct NodeConfig {
    /// Whether to run as a verifier
    is_verifier: bool,
    /// Network config
    network: NetworkConfig,
    /// Verifier config (if applicable)
    verifier: VerifierConfig,
    /// Data directory
    data_dir: String,
    /// Listen port
    port: u16,
    /// External address for NAT traversal
    external_addr: Option<String>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            is_verifier: false,
            network: NetworkConfig::default(),
            verifier: VerifierConfig::default(),
            data_dir: ".hardclaw".to_string(),
            port: 9000,
            external_addr: None,
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
    /// Token economics
    economics: Arc<RwLock<TokenEconomics>>,
    /// Verifier (if running as verifier)
    verifier: Option<Verifier>,
}

impl HardClawNode {
    /// Create a new node
    fn new(keypair: Keypair, config: NodeConfig) -> Self {
        let verifier = if config.is_verifier {
            Some(Verifier::new(
                Keypair::generate(),
                config.verifier.clone(),
            ))
        } else {
            None
        };

        Self {
            keypair,
            config,
            state: Arc::new(RwLock::new(ChainState::new())),
            mempool: Arc::new(RwLock::new(Mempool::new())),
            economics: Arc::new(RwLock::new(TokenEconomics::default())),
            verifier,
        }
    }

    /// Initialize the node
    async fn init(&mut self) -> anyhow::Result<()> {
        info!("Initializing HardClaw node...");

        // Initialize genesis block if needed
        let mut state = self.state.write().await;
        if state.height() == 0 {
            info!("Creating genesis block...");
            let genesis = Block::genesis(*self.keypair.public_key());
            state.apply_block(genesis)?;
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
            public_key: *self.keypair.public_key(),
            address: network_config.listen_addr.clone(),
            is_verifier: self.config.is_verifier,
            version: 1,
        };

        // Create network node
        let (mut network, mut event_rx) = NetworkNode::new(network_config, peer_info)?;

        let peer_id = network.local_peer_id();
        info!("P2P Peer ID: {}", peer_id);
        info!("Connect to this node with: /ip4/<IP>/tcp/{}/p2p/{}", self.config.port, peer_id);

        // Start network
        network.start().await?;

        if self.verifier.is_some() {
            info!("Running as verifier");
        } else {
            info!("Running as full node");
        }

        // Main event loop - handle network events and node logic together
        let is_verifier = self.verifier.is_some();
        loop {
            tokio::select! {
                // Handle network events
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
    async fn handle_network_event(&self, event: NetworkEvent) {
        match event {
            NetworkEvent::PeerConnected(peer) => {
                info!("Peer connected: {}", peer);
            }
            NetworkEvent::PeerDisconnected(peer) => {
                info!("Peer disconnected: {}", peer);
            }
            NetworkEvent::JobReceived(job) => {
                info!("Received job: {}", job.id);
                let mut mp = self.mempool.write().await;
                if let Err(e) = mp.add_job(job) {
                    warn!("Failed to add job to mempool: {}", e);
                }
            }
            NetworkEvent::SolutionReceived(solution) => {
                info!("Received solution: {}", solution.id);
            }
            NetworkEvent::BlockReceived(block) => {
                info!("Received block {} at height {}", block.hash, block.header.height);
                let mut st = self.state.write().await;
                if let Err(e) = st.apply_block(block) {
                    warn!("Failed to apply block: {}", e);
                }
            }
            NetworkEvent::AttestationReceived(attestation) => {
                info!("Received attestation for block {}", attestation.block_hash);
            }
            NetworkEvent::PeersDiscovered(peers) => {
                info!("Discovered {} peers via DHT", peers.len());
            }
            NetworkEvent::Started { peer_id, listen_addr } => {
                info!("Network started: {} @ {}", peer_id, listen_addr);
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
            info!("Produced block {} at height {}", block.hash, block.header.height);
            let mut state = self.state.write().await;
            state.apply_block(block)?;
        }

        Ok(())
    }

}

fn parse_args() -> NodeConfig {
    let args: Vec<String> = std::env::args().collect();
    let mut config = NodeConfig::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--verifier" | "-v" => config.is_verifier = true,
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
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }

    config
}

fn print_help() {
    println!("HardClaw Node");
    println!();
    println!("USAGE:");
    println!("    hardclaw-node [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -v, --verifier              Run as a verifier node");
    println!("    -p, --port <PORT>           Listen port (default: 9000)");
    println!("    -b, --bootstrap <ADDR>      Bootstrap peer address");
    println!("    --external-addr <ADDR>      External address for NAT traversal");
    println!("    --no-official-bootstrap     Don't use official bootstrap nodes");
    println!("    -h, --help                  Print help");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
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

    // Parse config
    let config = parse_args();

    // Generate or load keypair
    let keypair = Keypair::generate();
    let address = Address::from_public_key(keypair.public_key());

    info!("Node address: {}", address);

    // Create and run node
    let mut node = HardClawNode::new(keypair, config);
    node.init().await?;
    node.run().await?;

    Ok(())
}
