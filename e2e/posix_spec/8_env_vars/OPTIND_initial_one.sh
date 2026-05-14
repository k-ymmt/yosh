#!/bin/sh
# POSIX_REF: 8 Environment Variables - OPTIND
# DESCRIPTION: OPTIND starts at 1 at shell entry
# EXPECT_OUTPUT: 1
# EXPECT_EXIT: 0
echo "$OPTIND"
