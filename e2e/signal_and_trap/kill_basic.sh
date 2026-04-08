#!/bin/sh
# POSIX_REF: 4 Utilities - kill
# DESCRIPTION: kill sends signal to a process
# EXPECT_EXIT: 0
/bin/sleep 10 &
pid=$!
kill "$pid"
wait "$pid" 2>/dev/null
exit 0
