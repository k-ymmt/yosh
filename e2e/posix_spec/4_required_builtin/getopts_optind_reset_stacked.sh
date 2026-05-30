#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: OPTIND=1 restarts getopts parsing inside a stacked option
# EXPECT_OUTPUT: aa
# EXPECT_EXIT: 0
set -- -ab
getopts ab opt
printf '%s' "$opt"
OPTIND=1
getopts ab opt
printf '%s\n' "$opt"
