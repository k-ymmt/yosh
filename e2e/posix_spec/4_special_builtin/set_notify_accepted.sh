#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -b (asynchronous notification) is accepted; effects require job control
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
set -b
echo ok
