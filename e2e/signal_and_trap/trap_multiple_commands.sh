#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: Trap action can contain multiple commands
# EXPECT_OUTPUT<<END
# hello
# step1
# step2
# END
trap 'echo step1; echo step2' EXIT
echo hello
