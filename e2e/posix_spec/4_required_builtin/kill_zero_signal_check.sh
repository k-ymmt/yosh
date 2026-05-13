#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -0 PID tests whether PID can be signaled (no signal sent)
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill -0 "$pid"
status=$?
kill "$pid" 2>/dev/null
wait "$pid" 2>/dev/null
exit "$status"
