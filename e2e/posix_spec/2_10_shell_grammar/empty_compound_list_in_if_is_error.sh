#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: An empty compound_list inside 'if ... then fi' is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then
fi
