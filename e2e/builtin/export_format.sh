#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: export -p output is suitable for re-input
# EXPECT_EXIT: 0
export MY_TEST_EXPORT_VAR=hello
output=$(export -p)
case "$output" in
  *"export MY_TEST_EXPORT_VAR"*) exit 0 ;;
  *) exit 1 ;;
esac
