#!/bin/sh
# POSIX_REF: 4 Utilities - getopts
# DESCRIPTION: getopts indicates missing required arg (colon-prefix mode)
# EXPECT_OUTPUT: :a
# EXPECT_EXIT: 0
set -- -a
getopts ":a:" opt 2>/dev/null
echo "$opt$OPTARG"
