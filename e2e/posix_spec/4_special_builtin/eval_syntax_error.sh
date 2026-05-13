#!/bin/sh
# POSIX_REF: 2.14.6 eval
# DESCRIPTION: eval of a syntax error propagates non-zero exit
# EXPECT_EXIT: 2
eval "if then fi" 2>/dev/null
