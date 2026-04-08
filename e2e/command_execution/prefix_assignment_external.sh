#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Prefix assignment on external command does not persist
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
MY_PREFIX_TEST_VAR=hello /usr/bin/true
echo "$MY_PREFIX_TEST_VAR"
