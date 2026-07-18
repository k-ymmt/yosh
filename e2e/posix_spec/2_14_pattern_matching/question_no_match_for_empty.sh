#!/bin/sh
# POSIX_REF: 2.14 Pattern Matching Notation
# DESCRIPTION: ? does not match an empty string
# EXPECT_OUTPUT: none
# EXPECT_EXIT: 0
case "" in
    ?) echo one ;;
    *) echo none ;;
esac
