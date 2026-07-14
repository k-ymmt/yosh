#!/bin/sh
# POSIX_REF: 2.1 Shell Introduction
# DESCRIPTION: -c with a command_name operand sets $0 and later operands as positional parameters
# EXPECT_OUTPUT: name a
# EXPECT_EXIT: 0
./target/debug/yosh -c 'echo $0 $1' name a
