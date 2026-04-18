#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap on EXIT runs when the shell exits
# EXPECT_OUTPUT<<END
# before
# on_exit
# END
# EXPECT_EXIT: 0
trap 'echo on_exit' EXIT
echo before
