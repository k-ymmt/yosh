#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: '[!...]' is a negated bracket expression
# EXPECT_OUTPUT: not_in
# EXPECT_EXIT: 0
case z in [!abc]) echo not_in ;; *) echo in ;; esac
