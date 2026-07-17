#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands - for
# DESCRIPTION: do is an ordinary word inside a for word list
# EXPECT_OUTPUT: do
# EXPECT_EXIT: 0
for i in do; do echo $i; done
