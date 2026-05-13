#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Tilde in PATH is expanded once at assignment
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
HOME=/tmp
PATH=~/bin:$PATH
case "$PATH" in
    /tmp/bin:*) echo ok ;;
    *) echo "bad PATH: $PATH" ;;
esac
