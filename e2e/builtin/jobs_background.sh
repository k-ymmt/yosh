#!/bin/sh
# POSIX_REF: 2.11 Job Control
# DESCRIPTION: background job PID in $!, reaped by wait; non-interactive shells print no [n] pid notice (bash/dash parity)
# EXPECT_OUTPUT: done
# EXPECT_EXIT: 0
sleep 0.1 &
[ -n "$!" ] && wait "$!" && echo done
