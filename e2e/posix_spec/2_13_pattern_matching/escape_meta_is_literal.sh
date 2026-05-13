#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: Escaped pattern metacharacter is matched literally
# EXPECT_OUTPUT: lit
# EXPECT_EXIT: 0
case "*" in
    \*) echo lit ;;
    *) echo other ;;
esac
