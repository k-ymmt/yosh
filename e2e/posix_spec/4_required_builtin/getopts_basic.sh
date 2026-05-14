#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts a opt parses -a from $@
# EXPECT_OUTPUT: a
# EXPECT_EXIT: 0
set -- -a
getopts a opt
echo "$opt"
