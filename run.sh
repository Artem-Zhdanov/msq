#!/bin/bash
set -euo pipefail
set -x

if [ $# -lt 2 ]; then
    echo "Usage: $0  <role>< <instance>"
    exit 1
fi

ROLE="$1"
INSTANCE="$2"

OTEL_RESOURCE_ATTRIBUTES=service.namespace=abc,service.instance.id=instance-${INSTANCE} \
OTEL_EXPORTER_OTLP_METRICS_ENDPOINT=http://n25.servers.gosh.sh:4316 \
RUST_LOG=info  \
cargo run --bin ${ROLE} --release  -- --config test-configs/config-${INSTANCE}.yaml
