#!/bin/sh
# POSIX_REF: 2.2.4 Dollar-Single-Quotes
# DESCRIPTION: \xHH escapes denote raw bytes, so a UTF-8 byte sequence decodes as its character
# EXPECT_OUTPUT: 日
# EXPECT_EXIT: 0
printf '%s\n' $'\xe6\x97\xa5'
