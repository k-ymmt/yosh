#!/bin/sh
# POSIX_REF: 2.15 eval
# DESCRIPTION: eval with variable expansion constructs command dynamically
# EXPECT_OUTPUT: world
CMD='echo world'
eval $CMD
