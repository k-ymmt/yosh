#!/bin/sh
# POSIX_REF: 2.9.4.3 Case Conditional Construct
# DESCRIPTION: case with no match exits 0; with a match, the status of the last command
# EXPECT_OUTPUT<<END
# 0
# 1
# END
# EXPECT_EXIT: 0
case x in a) false ;; esac
echo $?
case a in a) false ;; esac
echo $?
