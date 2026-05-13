#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait after job has been collected returns the cached status
# EXPECT_OUTPUT<<END
# 5
# 5
# END
# EXPECT_EXIT: 0
sh -c 'exit 5' &
pid=$!
wait "$pid"
echo $?
wait "$pid"
echo $?
