#!/bin/sh
# POSIX_REF: 2.14 read
# DESCRIPTION: read preserves multi-byte UTF-8 input in assigned variables
# EXPECT_OUTPUT: café 日本語
# EXPECT_EXIT: 0
printf 'café 日本語\n' | {
    read a b
    echo "$a $b"
}
