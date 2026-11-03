#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: The delimiter is the word after quote removal only; $, ~ and ${} are kept literally
# EXPECT_OUTPUT<<END
# one
# two $x
# three
# four
# five
# six
# END
# EXPECT_EXIT: 0
x=val
cat <<$x
one
$x
cat <<"$x"
two $x
$x
cat <<~
three
~
cat <<a$b
four
a$b
cat <<${x}y
five
${x}y
cat <<a"b"'c'\d
six
abcd
