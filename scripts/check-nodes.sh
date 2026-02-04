#!/bin/bash

PROJECT="hardclaw"

echo "=== Bootstrap Node Status ==="
echo ""

for node in "bootstrap-us:us-central1-a" "bootstrap-eu:europe-west1-b"; do
    VM="${node%%:*}"
    ZONE="${node##*:}"

    echo "--- $VM ---"
    gcloud compute ssh $VM --project=$PROJECT --zone=$ZONE --command="
        echo 'Service status:'
        sudo systemctl is-active hardclaw 2>/dev/null || echo 'not running'
        echo ''
        echo 'Recent logs:'
        sudo journalctl -u hardclaw -n 10 --no-pager 2>/dev/null || echo 'no logs'
    " 2>/dev/null
    echo ""
done
