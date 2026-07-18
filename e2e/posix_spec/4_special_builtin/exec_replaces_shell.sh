#!/bin/sh
# POSIX_REF: 2.15 exec
# DESCRIPTION: exec with a command replaces the shell with the command
# EXPECT_OUTPUT: replaced
# EXPECT_EXIT: 0
exec sh -c 'echo replaced'
echo unreached
