#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: Trap action can contain multiple commands
# EXPECT_OUTPUT<<END
# hello
# step1
# step2
# END
trap 'echo step1; echo step2' EXIT
echo hello
