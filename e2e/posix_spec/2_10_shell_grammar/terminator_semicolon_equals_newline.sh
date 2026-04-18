#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: ';' and newline are interchangeable as list terminators
# EXPECT_OUTPUT<<END
# one
# two
# three
# END
# EXPECT_EXIT: 0
echo one; echo two
echo three
