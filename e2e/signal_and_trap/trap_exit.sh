#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: EXIT trap fires when shell exits
# EXPECT_OUTPUT<<END
# hello
# goodbye
# END
trap 'echo goodbye' EXIT
echo hello
