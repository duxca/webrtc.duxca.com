#!/bin/bash
set -euxo pipefail
cargo watch -x "run --features=local"

trap 'kill $(jobs -p) 2>/dev/null' EXIT
