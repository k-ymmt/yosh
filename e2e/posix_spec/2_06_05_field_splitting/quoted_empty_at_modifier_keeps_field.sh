#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: A quoted ${@...} modifier that expands to an empty string after an IFS delimiter keeps its empty field; only "$@" with no parameters vanishes
# EXPECT_OUTPUT<<END
# 2:[a][]
# 2:[a][]
# 2:[a][]
# 2:[a][]
# 2:[a][]
# 1:[a]
# 1:[a]
# 2:[a][]
# 3:[a][][]
# END
# EXPECT_EXIT: 0
x='a '
# One empty positional parameter: "${@:-}" substitutes the empty word.
set -- ''
set -- $x"${@:-}"; echo "$#:[$1][$2]"
set -- ''
set -- $x"$@"; echo "$#:[$1][$2]"
set -- ''
set -- $x"$*"; echo "$#:[$1][$2]"
# A set parameter with an empty alternative word.
set -- b
set -- $x"${@:+}"; echo "$#:[$1][$2]"
# No parameters: the modifier forms see @ unset and give a quoted empty
# string, while "$@" and "${@}" give no field at all.
set --
set -- $x"${@:-}"; echo "$#:[$1][$2]"
set --
set -- $x"$@"; echo "$#:[$1]"
set --
set -- $x"${@}"; echo "$#:[$1]"
set --
set -- $x"${@:-$@}"; echo "$#:[$1][$2]"
set -- '' ''
set -- $x"$@"; echo "$#:[$1][$2][$3]"
