#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: Glob with no matches returns the pattern literally
# EXPECT_OUTPUT: /tmp/yosh_nonexistent_glob_test_*.zzz
echo /tmp/yosh_nonexistent_glob_test_*.zzz
