#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: NUL bytes in command substitution output are dropped (dash, bash), so the value can reach external commands and the environment
# EXPECT_OUTPUT<<END
# 2
# 2
# ab
# matched
# x=ab
# END
# EXPECT_EXIT: 0
x=$(printf 'a\000b')
echo ${#x}
y=`printf '\000a\000b\000'`
echo ${#y}
/usr/bin/env printf '%s\n' "$x"
case $x in ab) echo matched;; *) echo nomatch;; esac
export x
/usr/bin/env | grep '^x='
