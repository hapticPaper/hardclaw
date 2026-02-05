//! Network layer for P2P communication.
//!
//! Uses libp2p for peer discovery and message propagation.
//! - Kademlia DHT for internet-wide peer discovery
//! - Gossipsub for message propagation
//! - mDNS for local network discovery (optional, for development)

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as StdHash, Hasher};
use std::time::Duration;

use hickory_resolver::{config::*, TokioAsyncResolver};
use libp2p::{
    futures::StreamExt,
    gossipsub::{self, IdentTopic, MessageAuthenticity, ValidationMode},
    identify,
    kad::{self, store::MemoryStore, Mode},
    mdns, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::crypto::{Hash, PublicKey};
use crate::types::{Block, JobPacket, SolutionCandidate, VerifierAttestation};

/// Protocol version string
const PROTOCOL_VERSION: &str = "/hardclaw/1.0.0";

/// Gossipsub topic for jobs
const TOPIC_JOBS: &str = "hardclaw/jobs";
/// Gossipsub topic for solutions
const TOPIC_SOLUTIONS: &str = "hardclaw/solutions";
/// Gossipsub topic for blocks
const TOPIC_BLOCKS: &str = "hardclaw/blocks";
/// Gossipsub topic for attestations
const TOPIC_ATTESTATIONS: &str = "hardclaw/attestations";

/// Official `HardClaw` bootstrap nodes using dnsaddr for dynamic peer ID resolution.
///
/// Peer IDs are stored in DNS TXT records at `_dnsaddr.<hostname>`, allowing
/// updates without code changes when bootstrap nodes restart.
pub const BOOTSTRAP_NODES: &[&str] = &[
    "/dnsaddr/bootstrap-us.clawpaper.com",
    "/dnsaddr/bootstrap-eu.clawpaper.com",
    "/dnsaddr/bootstrap-us2.clawpaper.com",
    "/dnsaddr/bootstrap-asia.clawpaper.com",
];

/// Network message types (serialized for gossipsub)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// New job announcement
    NewJob(JobPacket),
    /// New solution submission
    NewSolution(SolutionCandidate),
    /// New block proposal
    NewBlock(Block),
    /// Block attestation
    Attestation(VerifierAttestation),
    /// Request block by hash
    GetBlock(Hash),
    /// Request job by ID
    GetJob(Hash),
    /// Peer discovery
    PeerAnnounce(PeerInfo),
}

/// Information about a peer
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer's public key
    pub public_key: PublicKey,
    /// Network address
    pub address: String,
    /// Whether peer is a verifier
    pub is_verifier: bool,
    /// Protocol version
    pub version: u32,
}

/// Network configuration
#[derive(Clone, Debug)]
pub struct NetworkConfig {
    /// Listen address
    pub listen_addr: String,
    /// Bootstrap peers (in addition to official bootstrap nodes)
    pub bootstrap_peers: Vec<String>,
    /// Maximum connections
    pub max_connections: usize,
    /// Gossip message TTL
    pub gossip_ttl: u32,
    /// Enable mDNS for local peer discovery (useful for development)
    pub enable_mdns: bool,
    /// Use official bootstrap nodes
    pub use_official_bootstrap: bool,
    /// External address (for NAT traversal - your public IP)
    pub external_addr: Option<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addr: "/ip4/0.0.0.0/tcp/9000".to_string(),
            bootstrap_peers: Vec::new(),
            max_connections: 50,
            gossip_ttl: 10,
            enable_mdns: true,
            use_official_bootstrap: true,
            external_addr: None,
        }
    }
}

/// Events emitted by the network layer to the application
#[derive(Clone, Debug)]
pub enum NetworkEvent {
    /// New peer connected
    PeerConnected(PeerId),
    /// Peer disconnected
    PeerDisconnected(PeerId),
    /// Received a new job from the network
    JobReceived(JobPacket),
    /// Received a new solution from the network
    SolutionReceived(SolutionCandidate),
    /// Received a new block from the network
    BlockReceived(Block),
    /// Received an attestation from the network
    AttestationReceived(VerifierAttestation),
    /// Network started successfully
    Started {
        /// Our peer ID
        peer_id: PeerId,
        /// Address we're listening on
        listen_addr: Multiaddr,
    },
    /// Discovered new peers via DHT
    PeersDiscovered(Vec<PeerId>),
    /// Network error
    Error(String),
}

/// Combined network behaviour
#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "HardClawBehaviourEvent")]
struct HardClawBehaviour {
    /// Kademlia DHT for peer discovery
    kademlia: kad::Behaviour<MemoryStore>,
    /// Identify protocol for peer info exchange
    identify: identify::Behaviour,
    /// Gossipsub for pub/sub messaging
    gossipsub: gossipsub::Behaviour,
    /// mDNS for local peer discovery
    mdns: mdns::tokio::Behaviour,
}

/// Combined network behaviour event
#[derive(Debug)]
pub enum HardClawBehaviourEvent {
    /// Kademlia event
    Kademlia(kad::Event),
    /// Identify event
    Identify(identify::Event),
    /// Gossipsub event
    Gossipsub(gossipsub::Event),
    /// mDNS event
    Mdns(mdns::Event),
}

impl From<kad::Event> for HardClawBehaviourEvent {
    fn from(event: kad::Event) -> Self {
        Self::Kademlia(event)
    }
}

impl From<identify::Event> for HardClawBehaviourEvent {
    fn from(event: identify::Event) -> Self {
        Self::Identify(event)
    }
}

impl From<gossipsub::Event> for HardClawBehaviourEvent {
    fn from(event: gossipsub::Event) -> Self {
        Self::Gossipsub(event)
    }
}

impl From<mdns::Event> for HardClawBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        Self::Mdns(event)
    }
}

/// Network node with real libp2p implementation
pub struct NetworkNode {
    /// The libp2p swarm
    swarm: Swarm<HardClawBehaviour>,
    /// Configuration
    config: NetworkConfig,
    /// Our peer info
    _local_peer: PeerInfo,
    /// Channel to send events to the application
    event_tx: mpsc::Sender<NetworkEvent>,
    /// Topics we're subscribed to
    topics: Topics,
}

/// Gossipsub topics
struct Topics {
    jobs: IdentTopic,
    solutions: IdentTopic,
    blocks: IdentTopic,
    attestations: IdentTopic,
}

impl NetworkNode {
    /// Create a new network node
    ///
    /// # Errors
    /// Returns error if network initialization fails
    pub fn new(
        config: NetworkConfig,
        local_peer: PeerInfo,
    ) -> Result<(Self, mpsc::Receiver<NetworkEvent>), NetworkError> {
        // Derive libp2p keypair from wallet public key bytes
        // This ensures the peer ID is deterministic based on wallet identity
        let libp2p_keypair = derive_libp2p_keypair(&local_peer.public_key);

        Self::with_keypair(config, local_peer, libp2p_keypair)
    }

    /// Create a network node with a specific libp2p keypair
    ///
    /// # Errors
    /// Returns error if network initialization fails
    pub fn with_keypair(
        config: NetworkConfig,
        local_peer: PeerInfo,
        keypair: libp2p::identity::Keypair,
    ) -> Result<(Self, mpsc::Receiver<NetworkEvent>), NetworkError> {
        // Create event channel
        let (event_tx, event_rx) = mpsc::channel(1000);

        // Build the swarm with the provided identity
        let swarm = libp2p::SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| NetworkError::InitFailed(e.to_string()))?
            .with_dns()
            .map_err(|e| NetworkError::InitFailed(e.to_string()))?
            .with_behaviour(|key| {
                // Configure Kademlia DHT
                let peer_id = key.public().to_peer_id();
                let store = MemoryStore::new(peer_id);
                let mut kademlia = kad::Behaviour::new(peer_id, store);
                kademlia.set_mode(Some(Mode::Server));

                // Configure Identify protocol
                let identify = identify::Behaviour::new(identify::Config::new(
                    PROTOCOL_VERSION.to_string(),
                    key.public(),
                ));

                // Configure gossipsub
                let message_id_fn = |message: &gossipsub::Message| {
                    let mut hasher = DefaultHasher::new();
                    message.data.hash(&mut hasher);
                    message.topic.hash(&mut hasher);
                    gossipsub::MessageId::from(hasher.finish().to_string())
                };

                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(1))
                    .validation_mode(ValidationMode::Strict)
                    .message_id_fn(message_id_fn)
                    .build()
                    .map_err(std::io::Error::other)?;

                let gossipsub = gossipsub::Behaviour::new(
                    MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                )
                .map_err(std::io::Error::other)?;

                // Configure mDNS
                let mdns = mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    key.public().to_peer_id(),
                )?;

                Ok(HardClawBehaviour {
                    kademlia,
                    identify,
                    gossipsub,
                    mdns,
                })
            })
            .map_err(|e| NetworkError::InitFailed(e.to_string()))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        // Create topics
        let topics = Topics {
            jobs: IdentTopic::new(TOPIC_JOBS),
            solutions: IdentTopic::new(TOPIC_SOLUTIONS),
            blocks: IdentTopic::new(TOPIC_BLOCKS),
            attestations: IdentTopic::new(TOPIC_ATTESTATIONS),
        };

        Ok((
            Self {
                swarm,
                config,
                _local_peer: local_peer,
                event_tx,
                topics,
            },
            event_rx,
        ))
    }

    /// Get our peer ID
    #[must_use]
    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Start the network node
    ///
    /// # Errors
    /// Returns error if network initialization fails
    pub async fn start(&mut self) -> Result<(), NetworkError> {
        // Subscribe to all topics
        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topics.jobs)
            .map_err(|e| NetworkError::InitFailed(e.to_string()))?;

        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topics.solutions)
            .map_err(|e| NetworkError::InitFailed(e.to_string()))?;

        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topics.blocks)
            .map_err(|e| NetworkError::InitFailed(e.to_string()))?;

        self.swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&self.topics.attestations)
            .map_err(|e| NetworkError::InitFailed(e.to_string()))?;

        // Parse and listen on the configured address
        let listen_addr: Multiaddr = self
            .config
            .listen_addr
            .parse()
            .map_err(|e: libp2p::multiaddr::Error| NetworkError::InitFailed(e.to_string()))?;

        self.swarm
            .listen_on(listen_addr.clone())
            .map_err(|e| NetworkError::InitFailed(e.to_string()))?;

        // Add external address if configured (for NAT traversal)
        if let Some(ref ext_addr) = self.config.external_addr {
            if let Ok(addr) = ext_addr.parse::<Multiaddr>() {
                self.swarm.add_external_address(addr);
                info!(addr = %ext_addr, "Added external address for NAT traversal");
            }
        }

        info!(
            peer_id = %self.swarm.local_peer_id(),
            addr = %listen_addr,
            "Network node starting"
        );

        // Connect to official bootstrap nodes (resolve dnsaddr TXT records first)
        if self.config.use_official_bootstrap {
            for addr_str in BOOTSTRAP_NODES {
                for resolved in &resolve_dnsaddr(addr_str).await {
                    if let Err(e) = self.dial_and_add_to_dht(resolved) {
                        warn!(addr = %resolved, error = %e, "Failed to connect to official bootstrap node");
                    }
                }
            }
        }

        // Connect to user-specified bootstrap peers
        let bootstrap_peers = self.config.bootstrap_peers.clone();
        for peer_addr in &bootstrap_peers {
            for resolved in &resolve_dnsaddr(peer_addr).await {
                if let Err(e) = self.dial_and_add_to_dht(resolved) {
                    warn!(addr = %resolved, error = %e, "Failed to connect to bootstrap peer");
                }
            }
        }

        // Start DHT bootstrap process
        if let Err(e) = self.swarm.behaviour_mut().kademlia.bootstrap() {
            warn!(error = ?e, "Failed to start Kademlia bootstrap");
        }

        // Emit started event
        let _ = self
            .event_tx
            .send(NetworkEvent::Started {
                peer_id: *self.swarm.local_peer_id(),
                listen_addr,
            })
            .await;

        Ok(())
    }

    /// Dial a peer and add them to the DHT routing table
    fn dial_and_add_to_dht(&mut self, addr_str: &str) -> Result<(), NetworkError> {
        let addr: Multiaddr = addr_str
            .parse()
            .map_err(|e: libp2p::multiaddr::Error| NetworkError::ConnectionFailed(e.to_string()))?;

        // Extract peer ID from multiaddr if present
        if let Some(peer_id) = extract_peer_id(&addr) {
            // Add to Kademlia routing table
            self.swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, addr.clone());

            info!(peer = %peer_id, addr = %addr, "Added peer to DHT routing table");
        }

        // Dial the peer
        self.swarm
            .dial(addr)
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        Ok(())
    }

    /// Run the network event loop
    pub async fn run(&mut self) {
        // Periodic DHT refresh
        let mut dht_refresh_interval = tokio::time::interval(Duration::from_secs(300));

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }
                _ = dht_refresh_interval.tick() => {
                    // Periodically refresh DHT routing table
                    if let Err(e) = self.swarm.behaviour_mut().kademlia.bootstrap() {
                        debug!(error = ?e, "DHT bootstrap refresh failed (may be normal if no peers)");
                    }
                }
            }
        }
    }

    /// Process pending swarm events without blocking.
    /// Call this from an outer `select!` loop to drive the network.
    pub async fn poll(&mut self) {
        let event = self.swarm.select_next_some().await;
        self.handle_swarm_event(event).await;
    }

    /// Handle a swarm event
    async fn handle_swarm_event(&mut self, event: SwarmEvent<HardClawBehaviourEvent>) {
        match event {
            SwarmEvent::Behaviour(HardClawBehaviourEvent::Kademlia(
                kad::Event::RoutingUpdated {
                    peer, addresses, ..
                },
            )) => {
                info!(peer = %peer, addresses = ?addresses, "Kademlia routing table updated");
                // Add peer to gossipsub mesh
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .add_explicit_peer(&peer);
            }

            SwarmEvent::Behaviour(HardClawBehaviourEvent::Kademlia(
                kad::Event::OutboundQueryProgressed {
                    result: kad::QueryResult::GetClosestPeers(Ok(ok)),
                    ..
                },
            )) => {
                let peers: Vec<PeerId> = ok.peers.into_iter().collect();
                if !peers.is_empty() {
                    info!(count = peers.len(), "Discovered peers via DHT");
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::PeersDiscovered(peers))
                        .await;
                }
            }

            SwarmEvent::Behaviour(HardClawBehaviourEvent::Identify(
                identify::Event::Received { peer_id, info, .. },
            )) => {
                info!(
                    peer = %peer_id,
                    protocol_version = %info.protocol_version,
                    "Received identify info from peer"
                );
                // Add all peer addresses to Kademlia
                for addr in info.listen_addrs {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr);
                }
            }

            SwarmEvent::Behaviour(HardClawBehaviourEvent::Gossipsub(
                gossipsub::Event::Message {
                    propagation_source,
                    message_id,
                    message,
                },
            )) => {
                debug!(
                    source = %propagation_source,
                    id = %message_id,
                    topic = %message.topic,
                    "Received gossipsub message"
                );
                self.handle_gossipsub_message(&message).await;
            }

            SwarmEvent::Behaviour(HardClawBehaviourEvent::Mdns(mdns::Event::Discovered(peers))) => {
                for (peer_id, addr) in peers {
                    info!(peer = %peer_id, addr = %addr, "Discovered peer via mDNS");
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr);
                    self.swarm
                        .behaviour_mut()
                        .gossipsub
                        .add_explicit_peer(&peer_id);
                }
            }

            SwarmEvent::Behaviour(HardClawBehaviourEvent::Mdns(mdns::Event::Expired(peers))) => {
                for (peer_id, _addr) in peers {
                    debug!(peer = %peer_id, "mDNS peer expired");
                }
            }

            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                info!(peer = %peer_id, endpoint = ?endpoint, "Connection established");

                // When a connection is established, add the peer to Kademlia
                // This is crucial for bootstrap nodes discovered via DNS where the PeerID wasn't known initially
                if let libp2p::core::ConnectedPoint::Dialer { address, .. } = endpoint {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, address.clone());
                    // Trigger a bootstrap now that we have a peer
                    if let Err(e) = self.swarm.behaviour_mut().kademlia.bootstrap() {
                        debug!(error = ?e, "Bootstrap attempt after connection failed (may happen if only 1 peer)");
                    }
                } else if let libp2p::core::ConnectedPoint::Listener { send_back_addr, .. } =
                    endpoint
                {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, send_back_addr.clone());
                }

                let _ = self
                    .event_tx
                    .send(NetworkEvent::PeerConnected(peer_id))
                    .await;
            }

            SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                info!(peer = %peer_id, cause = ?cause, "Connection closed");
                let _ = self
                    .event_tx
                    .send(NetworkEvent::PeerDisconnected(peer_id))
                    .await;
            }

            SwarmEvent::NewListenAddr { address, .. } => {
                let peer_id = self.swarm.local_peer_id();
                info!(
                    addr = %address,
                    full_addr = %format!("{}/p2p/{}", address, peer_id),
                    "Listening on address"
                );
            }

            SwarmEvent::IncomingConnection { local_addr, .. } => {
                debug!(addr = %local_addr, "Incoming connection");
            }

            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                warn!(peer = ?peer_id, error = %error, "Outgoing connection error");
            }

            SwarmEvent::IncomingConnectionError {
                local_addr, error, ..
            } => {
                warn!(addr = %local_addr, error = %error, "Incoming connection error");
            }

            _ => {}
        }
    }

    /// Handle a gossipsub message
    async fn handle_gossipsub_message(&self, message: &gossipsub::Message) {
        let topic = message.topic.as_str();

        match topic {
            TOPIC_JOBS => {
                if let Ok(job) = bincode::deserialize::<JobPacket>(&message.data) {
                    debug!(job_id = %job.id, "Received job from network");
                    let _ = self.event_tx.send(NetworkEvent::JobReceived(job)).await;
                } else {
                    warn!("Failed to deserialize job message");
                }
            }
            TOPIC_SOLUTIONS => {
                if let Ok(solution) = bincode::deserialize::<SolutionCandidate>(&message.data) {
                    debug!(solution_id = %solution.id, "Received solution from network");
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::SolutionReceived(solution))
                        .await;
                } else {
                    warn!("Failed to deserialize solution message");
                }
            }
            TOPIC_BLOCKS => {
                if let Ok(block) = bincode::deserialize::<Block>(&message.data) {
                    debug!(block_hash = %block.hash, height = block.header.height, "Received block from network");
                    let _ = self.event_tx.send(NetworkEvent::BlockReceived(block)).await;
                } else {
                    warn!("Failed to deserialize block message");
                }
            }
            TOPIC_ATTESTATIONS => {
                if let Ok(attestation) = bincode::deserialize::<VerifierAttestation>(&message.data)
                {
                    debug!(block_hash = %attestation.block_hash, "Received attestation from network");
                    let _ = self
                        .event_tx
                        .send(NetworkEvent::AttestationReceived(attestation))
                        .await;
                } else {
                    warn!("Failed to deserialize attestation message");
                }
            }
            _ => {
                debug!(topic = %topic, "Unknown topic");
            }
        }
    }

    /// Connect to a peer
    pub fn connect(&mut self, addr: &str) -> Result<(), NetworkError> {
        self.dial_and_add_to_dht(addr)
    }

    /// Broadcast a job to the network
    pub fn broadcast_job(&mut self, job: &JobPacket) -> Result<(), NetworkError> {
        let data = bincode::serialize(job).map_err(|e| NetworkError::SendFailed(e.to_string()))?;

        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topics.jobs.clone(), data)
            .map_err(|e| NetworkError::SendFailed(e.to_string()))?;

        debug!(job_id = %job.id, "Broadcast job to network");
        Ok(())
    }

    /// Broadcast a solution to the network
    pub fn broadcast_solution(&mut self, solution: &SolutionCandidate) -> Result<(), NetworkError> {
        let data =
            bincode::serialize(solution).map_err(|e| NetworkError::SendFailed(e.to_string()))?;

        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topics.solutions.clone(), data)
            .map_err(|e| NetworkError::SendFailed(e.to_string()))?;

        debug!(solution_id = %solution.id, "Broadcast solution to network");
        Ok(())
    }

    /// Broadcast a block to the network
    pub fn broadcast_block(&mut self, block: &Block) -> Result<(), NetworkError> {
        let data =
            bincode::serialize(block).map_err(|e| NetworkError::SendFailed(e.to_string()))?;

        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topics.blocks.clone(), data)
            .map_err(|e| NetworkError::SendFailed(e.to_string()))?;

        debug!(block_hash = %block.hash, height = block.header.height, "Broadcast block to network");
        Ok(())
    }

    /// Broadcast an attestation to the network
    pub fn broadcast_attestation(
        &mut self,
        attestation: &VerifierAttestation,
    ) -> Result<(), NetworkError> {
        let data =
            bincode::serialize(attestation).map_err(|e| NetworkError::SendFailed(e.to_string()))?;

        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topics.attestations.clone(), data)
            .map_err(|e| NetworkError::SendFailed(e.to_string()))?;

        debug!(block_hash = %attestation.block_hash, "Broadcast attestation to network");
        Ok(())
    }

    /// Broadcast any network message (convenience method)
    ///
    /// # Errors
    /// Returns error if publishing to gossipsub fails.
    pub fn broadcast(&mut self, message: &NetworkMessage) -> Result<(), NetworkError> {
        match message {
            NetworkMessage::NewJob(job) => self.broadcast_job(job),
            NetworkMessage::NewSolution(solution) => self.broadcast_solution(solution),
            NetworkMessage::NewBlock(block) => self.broadcast_block(block),
            NetworkMessage::Attestation(attestation) => self.broadcast_attestation(attestation),
            _ => {
                debug!(message = ?message, "Unhandled broadcast message type");
                Ok(())
            }
        }
    }

    /// Get connected peer count
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.swarm.connected_peers().count()
    }

    /// Get list of connected peers
    #[must_use]
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.swarm.connected_peers().copied().collect()
    }

    /// Find peers close to a key in the DHT
    pub fn find_peers(&mut self, key: &[u8]) {
        self.swarm
            .behaviour_mut()
            .kademlia
            .get_closest_peers(key.to_vec());
    }
}

/// Derive a libp2p Ed25519 keypair deterministically from wallet public key.
/// This ensures the peer ID is stable as long as the wallet doesn't change.
fn derive_libp2p_keypair(wallet_pubkey: &crate::crypto::PublicKey) -> libp2p::identity::Keypair {
    use sha2::{Digest, Sha256};

    // Hash the wallet public key to get deterministic entropy for libp2p key
    let mut hasher = Sha256::new();
    hasher.update(b"hardclaw-libp2p-identity-v1");
    hasher.update(wallet_pubkey.as_bytes());
    let hash = hasher.finalize();

    // Use the hash as seed for libp2p Ed25519 keypair
    let mut hash_bytes: [u8; 32] = hash.into();
    let secret = libp2p::identity::ed25519::SecretKey::try_from_bytes(&mut hash_bytes)
        .expect("SHA-256 output is valid Ed25519 seed");
    let ed25519_keypair = libp2p::identity::ed25519::Keypair::from(secret);

    libp2p::identity::Keypair::from(ed25519_keypair)
}

/// Resolve `/dnsaddr/<hostname>` by looking up TXT records at `_dnsaddr.<hostname>`.
///
/// Returns the resolved multiaddrs (e.g. `/dns4/host/tcp/9000/p2p/<peer_id>`).
/// Non-dnsaddr addresses are returned as-is.
async fn resolve_dnsaddr(addr_str: &str) -> Vec<String> {
    let hostname = match addr_str.strip_prefix("/dnsaddr/") {
        Some(h) => h,
        None => return vec![addr_str.to_string()],
    };

    let lookup_name = format!("_dnsaddr.{hostname}");
    let resolver = TokioAsyncResolver::tokio(ResolverConfig::default(), ResolverOpts::default());

    let txt_records = match resolver.txt_lookup(&lookup_name).await {
        Ok(records) => records,
        Err(e) => {
            warn!(hostname = %hostname, error = %e, "Failed to resolve dnsaddr TXT records");
            return vec![addr_str.to_string()];
        }
    };

    let mut resolved = Vec::new();
    for record in txt_records {
        let txt = record.to_string();
        if let Some(multiaddr_str) = txt.strip_prefix("dnsaddr=") {
            info!(addr = %multiaddr_str, "Resolved bootstrap dnsaddr");
            resolved.push(multiaddr_str.to_string());
        }
    }

    if resolved.is_empty() {
        warn!(hostname = %hostname, "No dnsaddr TXT records found");
        vec![addr_str.to_string()]
    } else {
        resolved
    }
}

/// Extract peer ID from a multiaddr if it contains /p2p/<`peer_id`>
fn extract_peer_id(addr: &Multiaddr) -> Option<PeerId> {
    addr.iter().find_map(|p| {
        if let libp2p::multiaddr::Protocol::P2p(peer_id) = p {
            Some(peer_id)
        } else {
            None
        }
    })
}

/// Network errors
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// Initialization failed
    #[error("network initialization failed: {0}")]
    InitFailed(String),
    /// Connection failed
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
    /// Peer not found
    #[error("peer not found")]
    PeerNotFound,
    /// Message send failed
    #[error("failed to send message: {0}")]
    SendFailed(String),
    /// Network not started
    #[error("network not started")]
    NotStarted,
}

/// Network event handler trait
pub trait NetworkHandler: Send + Sync {
    /// Handle incoming job
    fn on_job(&mut self, job: JobPacket);
    /// Handle incoming solution
    fn on_solution(&mut self, solution: SolutionCandidate);
    /// Handle incoming block
    fn on_block(&mut self, block: Block);
    /// Handle incoming attestation
    fn on_attestation(&mut self, attestation: VerifierAttestation);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::default();
        assert_eq!(config.listen_addr, "/ip4/0.0.0.0/tcp/9000");
        assert!(config.bootstrap_peers.is_empty());
        assert_eq!(config.max_connections, 50);
        assert!(config.use_official_bootstrap);
    }

    #[tokio::test]
    async fn test_network_node_creation() {
        let config = NetworkConfig::default();
        let keypair = crate::crypto::Keypair::generate();
        let peer_info = PeerInfo {
            public_key: *keypair.public_key(),
            address: "/ip4/127.0.0.1/tcp/9000".to_string(),
            is_verifier: true,
            version: 1,
        };

        let result = NetworkNode::new(config, peer_info);
        assert!(result.is_ok());

        let (node, _rx) = result.unwrap();
        assert_eq!(node.peer_count(), 0);
    }

    #[test]
    fn test_message_serialization() {
        let keypair = crate::crypto::Keypair::generate();
        let block = Block::genesis(*keypair.public_key());

        let msg = NetworkMessage::NewBlock(block.clone());
        let serialized = bincode::serialize(&msg).unwrap();
        let deserialized: NetworkMessage = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            NetworkMessage::NewBlock(b) => {
                assert_eq!(b.hash, block.hash);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_extract_peer_id() {
        // Test with peer ID
        let addr: Multiaddr =
            "/ip4/127.0.0.1/tcp/9000/p2p/12D3KooWDpJ7As7BWAwRMfu1VU2WCqNjvq387JEYKDBj4kx6nXTN"
                .parse()
                .unwrap();
        let peer_id = extract_peer_id(&addr);
        assert!(peer_id.is_some());

        // Test without peer ID
        let addr_no_peer: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
        let no_peer_id = extract_peer_id(&addr_no_peer);
        assert!(no_peer_id.is_none());
    }
}
