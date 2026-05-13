#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: PATH is non-empty at shell startup
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
case "$PATH" in
    '') echo empty ;;
    *) echo ok ;;
esac
