#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Function defined inside another function
# EXPECT_OUTPUT<<END
# outer
# inner
# END
outer() {
  echo outer
  inner() { echo inner; }
  inner
}
outer
