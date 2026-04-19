#!/bin/sh
# POSIX_REF: 2.14.13 times
# DESCRIPTION: times works inside a subshell; output retains two-line shape
# EXPECT_OUTPUT omitted: CPU-time values are non-deterministic; shape verified in-script.
# EXPECT_EXIT: 0
out=$( ( times ) )
line1=$(echo "$out" | sed -n '1p')
line2=$(echo "$out" | sed -n '2p')
case "$line1" in
    *m*s\ *m*s) ;;
    *) echo "bad line1 in subshell: $line1" >&2; exit 1 ;;
esac
case "$line2" in
    *m*s\ *m*s) ;;
    *) echo "bad line2 in subshell: $line2" >&2; exit 1 ;;
esac
