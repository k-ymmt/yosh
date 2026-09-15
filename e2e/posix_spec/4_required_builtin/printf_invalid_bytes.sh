#!/bin/sh
# POSIX_REF: 4 Utilities - printf
# DESCRIPTION: printf passes non-UTF-8 bytes through operands and octal escapes unchanged
# EXPECT_OUTPUT: 2 ff ff
x=$(printf '\377' | od -An -tx1 | tr -d ' \n')
y=$(printf '%s' "$(printf '\377')" | od -An -tx1 | tr -d ' \n')
printf '%s %s %s\n' "$(printf '\377\376' | wc -c | tr -d ' ')" "$x" "$y"
