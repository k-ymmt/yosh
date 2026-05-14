#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts sets opt to ? for unknown options
# EXPECT_OUTPUT: ?
# EXPECT_EXIT: 0
set -- -x
getopts "a" opt 2>/dev/null
echo "$opt"
