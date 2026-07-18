#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A "shall exit" error in a subshell exits only the subshell; the parent continues
# EXPECT_OUTPUT<<END
# subshell-failed
# parent-alive
# END
# EXPECT_EXIT: 0
( echo "${x_unset_yosh:?}" ) 2>/dev/null
[ $? -ne 0 ] && echo subshell-failed
echo parent-alive
