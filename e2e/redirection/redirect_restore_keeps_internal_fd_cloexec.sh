#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Restoring a temporarily redirected internal fd (the signal self-pipe) keeps it close-on-exec, so a plain external command does not inherit it
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
trap 'echo trapped' USR1
: 10>&1 11>&1
/bin/sh -c 'n=$(/bin/ls /dev/fd | grep -c -E "^1[01]$"); echo "$n"'
