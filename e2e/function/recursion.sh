#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Recursive function calls
# EXPECT_OUTPUT<<END
# 3
# 2
# 1
# END
countdown() {
  if test "$1" -gt 0; then
    echo "$1"
    x=$1
    countdown $((x - 1))
  fi
}
countdown 3
