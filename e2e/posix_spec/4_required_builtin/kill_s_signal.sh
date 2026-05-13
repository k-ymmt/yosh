#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill -s TERM PID sends SIGTERM
# EXPECT_EXIT: 0
sleep 5 &
pid=$!
kill -s TERM "$pid"
wait "$pid" 2>/dev/null
exit 0
