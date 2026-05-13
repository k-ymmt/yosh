#!/bin/sh
# POSIX_REF: 2.14.15 times
# DESCRIPTION: times outputs four mm:ss.ff values on two lines (user/sys for shell, then children)
# EXPECT_EXIT: 0
out=$(times)
# Two lines, each has two mm:ss.ff values separated by whitespace
line_count=$(printf '%s\n' "$out" | wc -l)
[ "$line_count" -eq 2 ] || exit 1
