#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: && executes right side only when left side succeeds
# EXPECT_OUTPUT: yes
# EXPECT_EXIT: 0
true && echo yes
