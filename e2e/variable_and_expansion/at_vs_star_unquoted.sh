#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: Unquoted $@ should field-split each parameter independently
# XFAIL: unquoted $@ joins parameters with space instead of treating each independently
# EXPECT_EXIT: 0
# Test with custom IFS to expose the difference:
# POSIX: "a:b" split by IFS → "a","b", plus "c","d" = 4 fields
# kish bug: joins to "a:b c d" split by IFS → "a","b c d" = 2 fields
IFS=:
set -- "a:b" c d
count=0
for i in $@; do count=$((count + 1)); done
test "$count" = 4
