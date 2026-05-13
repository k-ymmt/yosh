#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -15 PID is equivalent to kill -TERM PID
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill -15 "$pid"
wait "$pid" 2>/dev/null
exit 0
