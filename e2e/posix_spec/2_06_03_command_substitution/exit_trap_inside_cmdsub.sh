#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: An EXIT trap set inside a command substitution runs when its subshell exits, and its output is captured
# EXPECT_OUTPUT<<END
# [t:1] 1
# [in]
# END
# EXPECT_EXIT: 0
v=$(trap 'echo t:$?' EXIT; false); echo "[$v] $?"
v=$(echo in); echo "[$v]"
