#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -h (locate utilities) is accepted without a diagnostic
# EXPECT_OUTPUT: no-error
# EXPECT_EXIT: 0
# XFAIL: yosh reports "unknown option: -h"
err=$(./target/debug/yosh -c 'set -h' 2>&1)
[ -z "$err" ] && echo no-error
