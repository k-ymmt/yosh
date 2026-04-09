#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: Adjacent quoted strings concatenate into one word
# EXPECT_OUTPUT: abc
echo 'a'"b"'c'
