#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: Unquoted $@ should produce separate fields per positional parameter
# XFAIL: unquoted $@ joins with space instead of producing separate fields
# EXPECT_OUTPUT<<END
# 3
# a b
# c
# d
# END
set -- "a b" c d
count=0
for i in $@; do count=$((count + 1)); done
echo "$count"
for i in "$@"; do echo "$i"; done
