#!/bin/sh
# POSIX_REF: 2.15 exit
# DESCRIPTION: exit with no operand returns the status of the last executed command
# EXPECT_EXIT: 1
false
exit
