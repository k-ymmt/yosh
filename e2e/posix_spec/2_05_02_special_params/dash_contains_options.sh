#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $- contains currently-set option letters
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
set -e
case "$-" in
    *e*) echo ok ;;
    *) echo "missing e in: $-" ;;
esac
