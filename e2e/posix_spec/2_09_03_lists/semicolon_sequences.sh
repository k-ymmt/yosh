#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: ; sequences commands left-to-right
# EXPECT_OUTPUT<<END
# a
# b
# END
# EXPECT_EXIT: 0
echo a; echo b
