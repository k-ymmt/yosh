#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill PID without -s defaults to SIGTERM
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill "$pid"
wait "$pid" 2>/dev/null
exit 0
