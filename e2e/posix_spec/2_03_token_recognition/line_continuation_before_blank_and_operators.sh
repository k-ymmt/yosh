#!/bin/sh
# POSIX_REF: 2.2.1 Escape Character (Backslash)
# DESCRIPTION: Backslash-newline is removed anywhere outside single quotes: before blanks, inside operators, after $, inside ${...}
# EXPECT_OUTPUT<<END
# a b
# x=1
# t
# a
# amp
# hd
# /hh
# c d
# XX
# qr s\
# t
# 3
# a\
# b
# END
HOME=/hh
echo a \
  b
x=1 \
  env | grep '^x='
if true; then \
  echo t; fi
echo a \
| cat
true &\
& echo amp
cat <\
<E
hd
E
echo ${HO\
ME}
echo `echo c \
 d`
x=XX; echo $\
x
echo "q\
r" 's\
t'
echo $((1\
+2))
echo a\\
echo b
