#!/bin/sh
# POSIX_REF: 2.10.2 Rule 9 - Body of compound_list (if)
# DESCRIPTION: An empty compound_list inside 'if ... then fi' is a syntax error
# EXPECT_EXIT: 2
# EXPECT_STDERR: syntax
if true; then
fi
