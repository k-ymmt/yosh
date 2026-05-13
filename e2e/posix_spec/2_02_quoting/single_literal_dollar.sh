#!/bin/sh
# POSIX_REF: 2.2.2 Single-Quotes
# DESCRIPTION: Single-quotes preserve literal $HOME
# EXPECT_OUTPUT: $HOME
# EXPECT_EXIT: 0
echo '$HOME'
