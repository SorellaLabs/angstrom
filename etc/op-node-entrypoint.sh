#!/bin/sh
set -eu

if [ -z "${OP_NODE_NETWORK:-}" ] && [ -z "${OP_NODE_ROLLUP_CONFIG:-}" ]; then
  echo "expected OP_NODE_NETWORK to be set" 1>&2
  exit 1
fi

echo "$OP_NODE_L2_ENGINE_AUTH_RAW" > "$OP_NODE_L2_ENGINE_AUTH"

exec op-node