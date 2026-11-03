#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: A stale mid-cluster position after the operands change restarts the element instead of crashing
# EXPECT_OUTPUT<<END
# a
# ? 2
# ? 2
# END
# EXPECT_EXIT: 0
getopts ab o -abc
echo "$o"
getopts ab o -x 2>/dev/null
echo "$o $OPTIND"
OPTIND=1
set -- -ab
getopts ab o
set -- -x
getopts ab o 2>/dev/null
echo "$o $OPTIND"
