#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: Function without return uses exit status of last command
# EXPECT_OUTPUT: 0
myfn() { true; }
myfn
echo "$?"
