#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS1
# DESCRIPTION: PS1 is not displayed in non-interactive shell (no stdout side-effect on script lines)
# EXPECT_OUTPUT: line1
# EXPECT_EXIT: 0
PS1='$ '
echo line1
