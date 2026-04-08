#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Background command with & and $! contains its PID
# XFAIL: Case pattern with [!range] causes panic in pattern matcher
# EXPECT_EXIT: 0
/bin/sleep 0 &
pid=$!
case "$pid" in
  ''|*[!0-9]*) exit 1 ;;
  *) exit 0 ;;
esac
