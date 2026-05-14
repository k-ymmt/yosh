#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts handles stacked single-letter options (-ab as -a -b)
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
set -- -ab
getopts ab opt
echo "$opt"
getopts ab opt
echo "$opt"
