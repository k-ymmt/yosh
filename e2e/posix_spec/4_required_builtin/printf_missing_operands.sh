#!/bin/sh
# POSIX_REF: 4 Utilities - printf
# DESCRIPTION: printf treats missing operands as empty strings or zero
# EXPECT_OUTPUT<<END
# |0|
# END
printf '%s|%d|\n'
