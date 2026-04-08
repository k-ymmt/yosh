#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Positional parameters are restored after function call
# EXPECT_OUTPUT<<END
# func: inner
# script: outer
# END
set -- outer
show() { echo "func: $1"; }
show inner
echo "script: $1"
