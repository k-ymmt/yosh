#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: [a-c] matches any character in the range a..c
# EXPECT_OUTPUT: in
# EXPECT_EXIT: 0
case b in
    [a-c]) echo in ;;
    *) echo out ;;
esac
