#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: set -C prevents > from overwriting existing files
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
cd "$TEST_TMPDIR"
echo a >f
set -C
echo b >f 2>/dev/null
