#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: A quoted '*' in a case pattern matches only a literal asterisk
# EXPECT_OUTPUT: literal_star
# EXPECT_EXIT: 0
case '*' in '*') echo literal_star ;; *) echo glob ;; esac
