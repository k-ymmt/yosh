#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $- holds current shell option flags
# EXPECT_EXIT: 0
# Set an option, then verify $- contains it
set -e
flags="$-"
case "$flags" in
  *e*) exit 0 ;;
  *) exit 1 ;;
esac
