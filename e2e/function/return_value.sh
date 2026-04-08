#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: return sets function exit status
# EXPECT_OUTPUT: 42
myfn() { return 42; }
myfn
echo "$?"
