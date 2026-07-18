#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -n reads commands but does not execute them
# EXPECT_OUTPUT:
# EXPECT_EXIT: 0
set -n
echo unreached
