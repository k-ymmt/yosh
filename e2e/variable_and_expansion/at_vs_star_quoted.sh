#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: "$@" vs "$*" — behavior difference in double quotes
# EXPECT_OUTPUT<<END
# 3
# 1
# END
set -- "a b" c d
count=0
for i in "$@"; do count=$((count + 1)); done
echo "$count"
count=0
for i in "$*"; do count=$((count + 1)); done
echo "$count"
