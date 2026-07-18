#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: 'trap - SIGNAL' resets the trap to default disposition
# EXPECT_OUTPUT<<END
# set
# reset
# END
# EXPECT_EXIT: 0
trap 'echo traphit' INT
echo set
trap - INT
echo reset
