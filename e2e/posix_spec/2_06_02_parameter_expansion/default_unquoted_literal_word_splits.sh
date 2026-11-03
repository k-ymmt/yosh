#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Unquoted literal text in the word of ${x:-w}, ${x:+w}, ${x=w} is field-split; quoted parts are not
# EXPECT_OUTPUT<<END
# 2
# 2
# 2 [a b]
# 2
# 2
# 1
# 3
# 1
# 2
# END
# EXPECT_EXIT: 0
unset x
set -- ${x:-a b}; echo $#
y=1
set -- ${y:+a b}; echo $#
set -- ${x=a b}; echo $# "[$x]"
unset x
IFS=:
set -- ${x:-a:b}; echo $#
unset IFS
set -- ${x:-a
b}; echo $#
set -- ${x:-"a b"}; echo $#
set -- ${x:-a "b c" d}; echo $#
set -- "${x:-a b}"; echo $#
set -- ${x:-${z:-a b}}; echo $#
