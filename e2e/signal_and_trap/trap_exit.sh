#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: EXIT trap fires when shell exits
# EXPECT_OUTPUT<<END
# hello
# goodbye
# END
trap 'echo goodbye' EXIT
echo hello
