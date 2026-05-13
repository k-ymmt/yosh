#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands
# DESCRIPTION: (...) runs commands in a subshell; assignments do not affect the parent
# EXPECT_OUTPUT: unset
# EXPECT_EXIT: 0
(x=value)
echo "${x:-unset}"
