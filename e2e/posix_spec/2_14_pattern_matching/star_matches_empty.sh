#!/bin/sh
# POSIX_REF: 2.14 Pattern Matching Notation
# DESCRIPTION: * matches the empty string
# EXPECT_OUTPUT: matched
# EXPECT_EXIT: 0
case "" in
    *) echo matched ;;
esac
