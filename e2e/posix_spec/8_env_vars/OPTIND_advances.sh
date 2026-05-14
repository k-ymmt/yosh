#!/bin/sh
# POSIX_REF: 8 Environment Variables - OPTIND
# DESCRIPTION: OPTIND advances as getopts consumes options
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
set -- -a
getopts a opt
echo "$OPTIND"
