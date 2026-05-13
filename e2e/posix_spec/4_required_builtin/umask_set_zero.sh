#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask 0 unmasks everything
# EXPECT_EXIT: 0
umask 0
out=$(umask)
case "$out" in
    0|00|000|0000) exit 0 ;;
    *) exit 1 ;;
esac
