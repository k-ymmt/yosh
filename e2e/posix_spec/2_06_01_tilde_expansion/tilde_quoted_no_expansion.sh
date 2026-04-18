#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: Quoted tilde is not expanded
# EXPECT_OUTPUT: ~
# EXPECT_EXIT: 0
echo '~'
