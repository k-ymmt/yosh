#!/bin/sh
# POSIX_REF: 4 Utilities - command
# DESCRIPTION: Use case - check whether a command is available before using it
# EXPECT_OUTPUT<<END
# sh available
# missing
# END
if command -v sh >/dev/null 2>&1; then
  echo "sh available"
fi
if command -v definitely_not_a_command_12345 >/dev/null 2>&1; then
  echo "found"
else
  echo "missing"
fi
