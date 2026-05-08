#!/bin/sh
# W-P3 — command-throughput workload for plugin perf measurement.
#
# Calls the perf_plugin `noop_cmd` 1000 times under a yosh `for` loop.
# Drive via:
#   yosh-dhat benches/data/plugin_w3.sh   (heap)
#   samply record -- yosh benches/data/plugin_w3.sh   (CPU)
#
# Requires perf_plugin to be enabled in the active plugins.lock. For
# isolation, wrap in `HOME=<staged-dir> ...`.

i=0
while [ "$i" -lt 1000 ]; do
    noop_cmd
    i=$((i + 1))
done
