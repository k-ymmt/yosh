#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask -S prints the mask symbolically (u=rwx,g=rx,o=rx)
# EXPECT_EXIT: 0
umask 022
umask -S | grep -q u= || exit 1
