#!/bin/sh
# POSIX_REF: 2.6.1 Tilde Expansion
# DESCRIPTION: A lone tilde with HOME empty yields an empty field that is removed (the word has no quotes)
# EXPECT_OUTPUT<<END
# 1
# [/a]
# 2
# 1
# END
# EXPECT_EXIT: 0
HOME=
set -- ~ ~/a
echo $#
printf '[%s]\n' "$@"
set -- ~"" x
echo $#
set -- ${unset_var:-~} x
echo $#
