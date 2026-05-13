#!/bin/sh
# POSIX_REF: 2.1 Shell Introduction
# DESCRIPTION: shell reads commands from stdin when no script operand is given
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
printf 'echo hi\n' | ./target/debug/yosh
