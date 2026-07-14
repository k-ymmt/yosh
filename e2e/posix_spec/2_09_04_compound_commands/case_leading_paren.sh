#!/bin/sh
# POSIX_REF: 2.9.4.3 Case Conditional Construct
# DESCRIPTION: A case item may have a leading ( before the pattern
# EXPECT_OUTPUT: matched
# EXPECT_EXIT: 0
case a in ( a ) echo matched ;; esac
