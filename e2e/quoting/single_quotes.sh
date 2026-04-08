#!/bin/sh
# POSIX_REF: 2.2.2 Single-Quotes
# DESCRIPTION: Single quotes preserve all characters literally
# EXPECT_OUTPUT: $HOME is literal
echo '$HOME is literal'
