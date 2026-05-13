#!/bin/sh
# POSIX_REF: 4 Utilities - fg
# DESCRIPTION: fg without monitor mode is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: fg
set +m
fg >/dev/null
