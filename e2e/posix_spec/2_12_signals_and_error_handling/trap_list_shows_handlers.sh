#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: trap with no arguments lists currently-set handlers
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
trap 'echo bye' INT
out=$(trap)
case "$out" in
    *INT*) echo ok ;;
    *) echo "missing: $out" ;;
esac
