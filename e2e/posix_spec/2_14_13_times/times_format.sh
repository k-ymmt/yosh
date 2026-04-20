#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times prints two lines in "NmS.sssS NmS.sssS" shape
# EXPECT_OUTPUT omitted: CPU-time values are non-deterministic; shape verified in-script.
# EXPECT_EXIT: 0
out=$(times)
line1=$(echo "$out" | sed -n '1p')
line2=$(echo "$out" | sed -n '2p')
case "$line1" in
    [0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s) ;;
    *) echo "bad line1: $line1" >&2; exit 1 ;;
esac
case "$line2" in
    [0-9]*m[0-9]*.[0-9]*s\ [0-9]*m[0-9]*.[0-9]*s) ;;
    *) echo "bad line2: $line2" >&2; exit 1 ;;
esac
