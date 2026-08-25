#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Set ${@:-w}/${*:-w} keep field-per-parameter shape; ${#?} is length of $?
# EXPECT_OUTPUT<<END
# <a b>
# <c>
# 1
# <a>
# <b>
# <a b>
# <a,b>
# <a,b>
# 1:post
# <a,b>
# END
set -- "a b" c
printf "<%s>\n" "${@:-x}"
false
printf "%s\n" "${#?}"
set -- a b
IFS=
printf "<%s>\n" ${*:-x}
unset IFS
printf "<%s>\n" "${*:?err}"
IFS=,
x=${*:-fallback}
printf "<%s>\n" "$x"
y=$*
printf "<%s>\n" "$y"
set --
set -- "$@"post
printf "%s:%s\n" "$#" "$1"
set -- a b
unset z
printf "<%s>\n" "${z:=$*}"
