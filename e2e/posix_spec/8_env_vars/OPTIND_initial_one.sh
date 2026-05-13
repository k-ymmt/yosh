#!/bin/sh
# POSIX_REF: 8 Environment Variables - OPTIND
# DESCRIPTION: OPTIND starts at 1 at shell entry
# XFAIL: OPTIND default-init requires getopts builtin to be implemented (TODO: implement getopts)
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
echo "$OPTIND"
