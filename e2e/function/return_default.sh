#!/bin/sh
# POSIX_REF: 2.15 return
# DESCRIPTION: Function without return uses exit status of last command
# EXPECT_OUTPUT: 0
myfn() { true; }
myfn
echo "$?"
