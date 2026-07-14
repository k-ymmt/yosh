#!/bin/sh
# POSIX_REF: 2.15 exit
# DESCRIPTION: exit inside an EXIT trap action exits immediately with the given status
# EXPECT_OUTPUT: 7
# EXPECT_EXIT: 0
./target/debug/yosh -c 'trap "exit 7" EXIT; exit 3'
echo $?
