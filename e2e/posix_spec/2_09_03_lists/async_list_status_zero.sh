#!/bin/sh
# POSIX_REF: 2.9.3.1 Asynchronous Lists
# DESCRIPTION: The exit status of an asynchronous list is zero
# EXPECT_OUTPUT: 0
# EXPECT_EXIT: 0
false &
echo $?
wait "$!"
:
