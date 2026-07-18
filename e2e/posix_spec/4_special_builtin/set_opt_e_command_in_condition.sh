#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -e does NOT exit when command status is tested in if/while/until
# EXPECT_OUTPUT: tested
# EXPECT_EXIT: 0
set -e
if false; then echo unreached; fi
echo tested
