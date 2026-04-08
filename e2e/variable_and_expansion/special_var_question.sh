#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $? holds exit status of last command
# EXPECT_OUTPUT<<END
# 0
# 1
# END
true
echo "$?"
false
echo "$?"
