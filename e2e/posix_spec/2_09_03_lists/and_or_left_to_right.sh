#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: && and || have equal precedence and associate left-to-right
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
true || false && echo x
