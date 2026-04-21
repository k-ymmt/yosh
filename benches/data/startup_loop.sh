#!/bin/sh
# startup_loop.sh — W1 loop wrapper for samply.
# Invokes yosh N times so that samply has enough samples to resolve
# short-lived startup costs.
#
# Usage: startup_loop.sh <yosh-binary> [N]

set -eu

YOSH=${1:?"missing yosh binary path"}
N=${2:-1000}

i=0
while [ "$i" -lt "$N" ]; do
    "$YOSH" -c 'echo hi' > /dev/null
    i=$((i + 1))
done
