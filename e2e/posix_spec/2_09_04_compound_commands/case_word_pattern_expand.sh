#!/bin/sh
# POSIX_REF: 2.9.4.3 Case Conditional Construct
# DESCRIPTION: The case word and the patterns both undergo expansion
# EXPECT_OUTPUT<<END
# word-expanded
# pattern-expanded
# END
# EXPECT_EXIT: 0
w=hello
case $w in hello) echo word-expanded ;; esac
p="h*"
case hello in $p) echo pattern-expanded ;; esac
