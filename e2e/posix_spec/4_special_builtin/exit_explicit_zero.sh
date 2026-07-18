#!/bin/sh
# POSIX_REF: 2.15 exit
# DESCRIPTION: exit 0 forces exit status 0 regardless of prior command
# EXPECT_EXIT: 0
false
exit 0
