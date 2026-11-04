#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: EXIT trap run after a syntax error sees $? equal to the shell's exit status 2
# EXPECT_OUTPUT<<END
# before
# exit-status=2
# END
# EXPECT_EXIT: 2
trap 'echo "exit-status=$?"' EXIT
echo before
if
