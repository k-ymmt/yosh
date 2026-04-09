#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $- holds current shell option flags
# EXPECT_EXIT: 0
flags="$-"
test -n "$flags"
