#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: AND list - second command runs only if first succeeds
# EXPECT_OUTPUT: yes
# EXPECT_EXIT: 1
true && echo yes
false && echo no
