#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Prefix/suffix removal with bracket, ? and multi-star patterns picks the shortest/longest match
# EXPECT_OUTPUT<<END
# b/c.d/e
# e
# a/b/c.d
# a
# /c.d/e
# a/b/c.d/
# c.d/e
# 
# a/b/c.d/e
# 語c
# 日本
# END
v=a/b/c.d/e
echo "${v#*[/]}"
echo "${v##*[/]}"
echo "${v%[/]*}"
echo "${v%%[/]*}"
echo "${v#a?b}"
echo "${v%?}"
echo "${v#*/*[/]}"
echo "${v%%?[/]*}"
echo "${v#*x*}"
w=日本語c
echo "${w#*[本]}"
echo "${w%?[c]}"
