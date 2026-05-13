#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts advances OPTIND across options
# XFAIL: not yet implemented (TODO: implement getopts)
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
set -- -a
getopts a opt
echo "$OPTIND"
