//! Minimal HTTP API for HardClaw Node
//! Handles zero-dependency HTTP parsing to avoid bloating the binary.

use serde_json::json;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::crypto::Hash;
use crate::mempool::Mempool;
use crate::state::ChainState;
use crate::types::Address;

const EXPLORER_HTML: &str = include_str!("explorer.html");

/// Start the API server in a background task
pub async fn start_api_server(
    state: Arc<RwLock<ChainState>>,
    mempool: Arc<RwLock<Mempool>>,
    port: u16,
) {
    let addr = format!("0.0.0.0:{}", port);
    info!("Starting Endpoint at http://{}", addr);

    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind API port {}: {}", port, e);
            return;
        }
    };

    loop {
        let (mut socket, _) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                warn!("API accept error: {}", e);
                continue;
            }
        };

        let state = state.clone();
        let mempool = mempool.clone();

        tokio::spawn(async move {
            let mut buf = [0; 4096];
            let n = match socket.read(&mut buf).await {
                Ok(0) => return,
                Ok(n) => n,
                Err(_) => return,
            };

            let request = String::from_utf8_lossy(&buf[..n]);
            let response = handle_request(&request, &state, &mempool).await;

            if let Err(e) = socket.write_all(response.as_bytes()).await {
                warn!("Failed to write API response: {}", e);
            }
        });
    }
}

async fn handle_request(
    req: &str,
    state: &Arc<RwLock<ChainState>>,
    mempool: &Arc<RwLock<Mempool>>,
) -> String {
    let first_line = req.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");

    if method != "GET" {
        return not_found();
    }

    if path == "/" || path == "/index.html" {
        return html_response(EXPLORER_HTML);
    }

    if path == "/api/status" {
        let (height, chain_id, tip) = {
            let st = state.read().await;
            (
                st.height(),
                st.chain_id().map(ToString::to_string),
                st.tip().map(|b| b.hash.to_string()),
            )
        };

        let mp_size = mempool.read().await.size();

        return json_response(json!({
            "height": height,
            "chain_id": chain_id,
            "tip": tip,
            "mempool_size": mp_size.jobs + mp_size.solutions,
            "peer_count": 0
        }));
    }

    if path == "/api/blocks/recent" {
        let blocks = {
            let st = state.read().await;
            let height = st.height(); // block count (1 after genesis)
            let mut b = Vec::new();

            if height > 0 {
                let max_block = height - 1; // 0-indexed block heights
                let start = max_block.saturating_sub(9);

                for h in (start..=max_block).rev() {
                    if let Some(block) = st.get_block_at_height(h) {
                        let has_genesis = block.genesis_job.is_some();
                        b.push(json!({
                            "height": block.header.height,
                            "hash": block.hash.to_string(),
                            "parent_hash": block.header.parent_hash.to_string(),
                            "tx_count": block.verifications.len(),
                            "timestamp": block.header.timestamp,
                            "is_genesis": has_genesis
                        }));
                    }
                }
            }
            b
        };
        return json_response(json!(blocks));
    }

    if path.starts_with("/api/balance/") {
        let addr_str = path.trim_start_matches("/api/balance/");
        // Simple hex parsing for address
        if let Ok(bytes) = hex::decode(addr_str.trim_start_matches("0x")) {
            if bytes.len() == 20 {
                let mut arr = [0u8; 20];
                arr.copy_from_slice(&bytes);
                let address = Address::from_bytes(arr);
                let balance = state.read().await.balance_of(&address);
                return json_response(json!({
                    "address": addr_str,
                    "balance": balance.whole_hclaw(),
                    "raw": balance.raw()
                }));
            }
        }
        return json_response(json!({ "error": "Invalid address" }));
    }

    if path.starts_with("/api/block/") {
        let query = path.trim_start_matches("/api/block/");

        // Try by hash first
        if let Ok(hash) = Hash::from_hex(query) {
            let block = state.read().await.get_block(&hash).cloned();
            if let Some(b) = block {
                return json_response(json!(b));
            }
        }

        // Try by height
        if let Ok(height) = query.parse::<u64>() {
            let block = state.read().await.get_block_at_height(height).cloned();
            if let Some(b) = block {
                return json_response(json!(b));
            }
        }

        return json_response(json!({ "error": "Block not found" }));
    }

    if path.starts_with("/api/job/") {
        let query = path.trim_start_matches("/api/job/");
        if let Ok(hash) = Hash::from_hex(query) {
            let job = state.read().await.get_job(&hash).cloned();
            if let Some(j) = job {
                return json_response(json!(j));
            }
        }
        return json_response(json!({ "error": "Job not found" }));
    }

    // Genesis info endpoint - returns genesis block details
    if path == "/api/genesis" {
        let result = {
            let st = state.read().await;
            st.get_block_at_height(0).map(|block| {
                let genesis_job = block.genesis_job.as_ref().map(|job| {
                    json!({
                        "id": job.id.to_string(),
                        "description": job.description,
                        "job_type": format!("{:?}", job.job_type),
                        "status": format!("{:?}", job.status),
                        "bounty": job.bounty.whole_hclaw(),
                        "created_at": job.created_at
                    })
                });
                json!({
                    "height": 0,
                    "hash": block.hash.to_string(),
                    "proposer": block.header.proposer.to_hex(),
                    "timestamp": block.header.timestamp,
                    "genesis_job": genesis_job
                })
            })
        };
        return match result {
            Some(data) => json_response(data),
            None => json_response(json!({ "error": "Genesis block not found" })),
        };
    }

    not_found()
}

fn html_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn json_response(body: serde_json::Value) -> String {
    let s = body.to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        s.len(),
        s
    )
}

fn not_found() -> String {
    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
}
