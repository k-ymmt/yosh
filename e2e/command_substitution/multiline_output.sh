#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Multi-line command substitution output preserved (minus trailing newlines)
# EXPECT_OUTPUT<<END
# line1
# line2
# END
x=$(printf 'line1\nline2\n')
echo "$x"
