#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: AND list short-circuits on failure
# EXPECT_OUTPUT: done
false && echo "should not print"
echo done
