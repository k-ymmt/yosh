#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask with no operand prints the current mask in octal
# EXPECT_EXIT: 0
umask | grep -qE '^[0-7]+$' || exit 1
