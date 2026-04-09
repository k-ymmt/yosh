#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: OR list short-circuits on success
# EXPECT_OUTPUT: first
true || echo "should not print"
echo first
