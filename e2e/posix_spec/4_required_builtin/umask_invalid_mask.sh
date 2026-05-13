#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask with an invalid mask is an error
# EXPECT_EXIT: 2
# EXPECT_STDERR: umask
umask 999
