#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: exit inside function terminates the entire shell
# EXPECT_EXIT: 42
myfunc() {
  exit 42
}
myfunc
echo "should not reach here"
