#!/bin/bash
set -e

# Reset HardClaw chain state and re-initialize from genesis.
#
# This wipes local chain data so the node starts fresh from block 0.
# The wallet (seed phrase) is preserved.
#
# Usage:
#   ./scripts/reset-genesis.sh                        # reset default local chain
#   ./scripts/reset-genesis.sh --chain-id <ID>        # reset a specific chain
#   ./scripts/reset-genesis.sh --all                  # reset ALL chains

DATA_DIR="${HARDCLAW_DATA_DIR:-$HOME/.hardclaw}"
CHAIN_ID=""
RESET_ALL=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --chain-id)
            CHAIN_ID="$2"
            shift 2
            ;;
        --all)
            RESET_ALL=true
            shift
            ;;
        --data-dir)
            DATA_DIR="$2"
            shift 2
            ;;
        -h|--help)
            echo "Reset HardClaw chain state (preserves wallet)"
            echo ""
            echo "Usage:"
            echo "  $0                          Reset default local chain data"
            echo "  $0 --chain-id <ID>          Reset a specific chain"
            echo "  $0 --all                    Reset ALL chains"
            echo "  $0 --data-dir <PATH>        Custom data directory (default: ~/.hardclaw)"
            echo ""
            echo "This removes chain state so the node re-initializes from genesis."
            echo "Your wallet seed phrase is NOT affected."
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Safety check: refuse mainnet without explicit confirmation
if [[ "$CHAIN_ID" == hardclaw-mainnet* ]]; then
    echo "WARNING: You are about to reset mainnet chain '$CHAIN_ID'."
    echo "This will destroy all local chain state. Your wallet is preserved."
    read -rp "Type 'yes' to confirm: " confirm
    if [[ "$confirm" != "yes" ]]; then
        echo "Aborted."
        exit 1
    fi
fi

CHAINS_DIR="$DATA_DIR/chains"

if $RESET_ALL; then
    if [[ -d "$CHAINS_DIR" ]]; then
        echo "Removing all chain data in $CHAINS_DIR ..."
        rm -rf "$CHAINS_DIR"
        echo "Done. All chain state has been wiped."
    else
        echo "No chain data found at $CHAINS_DIR"
    fi
elif [[ -n "$CHAIN_ID" ]]; then
    CHAIN_DIR="$CHAINS_DIR/$CHAIN_ID"
    if [[ -d "$CHAIN_DIR" ]]; then
        echo "Removing chain data for '$CHAIN_ID' ..."
        rm -rf "$CHAIN_DIR"
        echo "Done. Chain '$CHAIN_ID' state has been wiped."
    else
        echo "No chain data found for '$CHAIN_ID' at $CHAIN_DIR"
    fi
else
    # Default: remove all chain data (most users run one chain)
    if [[ -d "$CHAINS_DIR" ]]; then
        echo "Removing all chain data in $CHAINS_DIR ..."
        rm -rf "$CHAINS_DIR"
        echo "Done. All chain state has been wiped."
    else
        echo "No chain data found at $CHAINS_DIR"
    fi
fi

echo ""
echo "Wallet seed phrase preserved at $DATA_DIR/seed_phrase.txt"
echo "Restart the node to re-initialize from genesis."
