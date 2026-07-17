#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Use case - recursive function computing factorial via command substitution
# EXPECT_OUTPUT: 120
fact() {
  if [ "$1" -le 1 ]; then
    echo 1
  else
    prev=$(fact $(( $1 - 1 )))
    echo $(( $1 * prev ))
  fi
}
fact 5
