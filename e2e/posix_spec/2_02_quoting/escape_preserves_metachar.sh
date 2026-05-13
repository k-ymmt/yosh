#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character (Backslash)
# DESCRIPTION: Backslash preserves literal value of glob metacharacter
# EXPECT_OUTPUT: *
# EXPECT_EXIT: 0
echo \*
