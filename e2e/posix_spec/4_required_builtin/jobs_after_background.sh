#!/bin/sh
# POSIX_REF: 4 Utilities - jobs
# DESCRIPTION: jobs lists background jobs running in the current shell
# EXPECT_EXIT: 0
set -m 2>/dev/null
sleep 0.1 &
out=$(jobs)
wait
case "$out" in
    *sleep*) exit 0 ;;
    *) exit 1 ;;
esac
