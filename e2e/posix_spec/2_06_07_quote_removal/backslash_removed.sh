#!/bin/sh
# POSIX_REF: 2.6.7 Quote Removal
# DESCRIPTION: Backslash escape character is removed from final word
# EXPECT_OUTPUT: $
# EXPECT_EXIT: 0
echo \$
