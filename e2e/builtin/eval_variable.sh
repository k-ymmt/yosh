#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: eval with variable expansion constructs command dynamically
# EXPECT_OUTPUT: world
CMD='echo world'
eval $CMD
