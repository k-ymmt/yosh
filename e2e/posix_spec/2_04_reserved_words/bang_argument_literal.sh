#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: ! is a reserved word only in command position; as an argument it is literal
# EXPECT_OUTPUT: !
# EXPECT_EXIT: 0
echo !
