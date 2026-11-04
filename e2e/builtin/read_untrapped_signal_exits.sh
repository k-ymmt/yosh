#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: An untrapped SIGTERM ends a shell blocked in read on idle input right away, not when input arrives
# EXPECT_OUTPUT: waiting
# EXPECT_EXIT: 143
cd "$TEST_TMPDIR" || exit 1
mkfifo idle || exit 1
# Read-write open: a FIFO with a writer that never writes, so read blocks.
exec 3<>idle
echo waiting
(sleep 1; kill -TERM $$) &
read x <&3
echo "not reached"
