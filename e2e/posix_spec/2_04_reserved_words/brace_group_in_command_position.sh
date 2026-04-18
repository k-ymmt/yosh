#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: '{' and '}' act as grouping reserved words in command position
# EXPECT_OUTPUT: grouped
# EXPECT_EXIT: 0
{ echo grouped; }
