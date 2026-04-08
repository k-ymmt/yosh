#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Assignment within arithmetic expression persists
# EXPECT_OUTPUT<<END
# 42
# 42
# END
echo $((x = 42))
echo "$x"
