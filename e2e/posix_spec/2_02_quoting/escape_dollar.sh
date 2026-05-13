#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character (Backslash)
# DESCRIPTION: Backslash preserves literal value of dollar sign
# EXPECT_OUTPUT: $HOME
# EXPECT_EXIT: 0
echo \$HOME
