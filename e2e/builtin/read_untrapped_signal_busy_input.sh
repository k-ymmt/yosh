#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: An untrapped SIGTERM ends a shell whose read is consuming endless input without a newline
# EXPECT_OUTPUT: reading
# EXPECT_EXIT: 143
# /dev/zero is always readable and never yields a newline, so read never
# blocks and never completes: the signal must be noticed between reads.
echo reading
(sleep 1; kill -TERM $$) &
read -r x </dev/zero
echo "not reached"
