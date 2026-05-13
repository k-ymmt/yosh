#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: Word expansion occurs before the command is looked up and executed
# EXPECT_OUTPUT: hi
# EXPECT_EXIT: 0
cmd=echo
$cmd hi
