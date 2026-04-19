#!/bin/sh
# POSIX_REF: 2.10.2 Rule 10 - Keyword recognition
# DESCRIPTION: Reserved word is recognized in command position after a pipe
# EXPECT_OUTPUT: x
# EXPECT_EXIT: 0
echo x | if true; then cat; fi
