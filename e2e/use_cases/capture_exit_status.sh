#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters - ?
# DESCRIPTION: Use case - capture $? and dispatch on a command's exit status
# EXPECT_OUTPUT<<END
# command failed with status 1
# command succeeded
# END
false
status=$?
case $status in
  0) echo "command succeeded" ;;
  *) echo "command failed with status $status" ;;
esac
true
status=$?
case $status in
  0) echo "command succeeded" ;;
  *) echo "command failed with status $status" ;;
esac
