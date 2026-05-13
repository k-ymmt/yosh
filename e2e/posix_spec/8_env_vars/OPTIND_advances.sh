#!/bin/sh
# POSIX_REF: 8 Environment Variables - OPTIND
# DESCRIPTION: OPTIND advances as getopts consumes options
# XFAIL: not yet implemented (TODO: implement getopts; OPTIND advance requires native getopts)
# EXPECT_OUTPUT: 2
# EXPECT_EXIT: 0
set -- -a
getopts a opt
echo "$OPTIND"
