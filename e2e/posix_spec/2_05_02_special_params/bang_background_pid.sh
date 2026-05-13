#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $! is the PID of the most recent background command
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
sleep 0 &
case "$!" in
    ''|*[!0-9]*) echo "bad bang: $!" ;;
    *) echo ok ;;
esac
wait
