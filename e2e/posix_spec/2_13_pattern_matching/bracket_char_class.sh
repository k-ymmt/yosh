#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: Bracket expression matches any contained char
# EXPECT_OUTPUT: match
# EXPECT_EXIT: 0
case b in [abc]) echo match ;; *) echo no ;; esac
