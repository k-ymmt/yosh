#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved word after | is recognized as command-position start
# EXPECT_OUTPUT: looped
# EXPECT_EXIT: 0
echo a | while read x; do echo looped; done
