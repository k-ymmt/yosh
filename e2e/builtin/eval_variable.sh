#!/bin/sh
# POSIX_REF: 2.14.5 eval
# DESCRIPTION: eval with variable expansion constructs command dynamically
# EXPECT_OUTPUT: world
CMD='echo world'
eval $CMD
