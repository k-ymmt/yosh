#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: exit without an operand inside a trap action uses the pre-trap $?
# EXPECT_OUTPUT: status-preserved
# EXPECT_EXIT: 0
./target/debug/yosh -c 'trap "true; exit" EXIT; false'
[ $? -eq 1 ] && echo status-preserved
