#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: || executes right side only when left side fails
# EXPECT_OUTPUT: no
# EXPECT_EXIT: 0
false || echo no
