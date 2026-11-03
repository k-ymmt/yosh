#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${@#pat} / ${*%pat} remove the pattern from each positional parameter and keep the $@ / $* field shape (unspecified by POSIX; bash)
# EXPECT_OUTPUT<<END
# [ b][c]
# [d1][d2][f]
# [q][s]
# [b:c]
# b c
# [x]
# END
# EXPECT_EXIT: 0
set -- 'a b' c
printf '[%s]' "${@#a}"; echo
set -- d1/ d2/ f
printf '[%s]' "${@%/}"; echo
set -- p/q r/s
printf '[%s]' ${@##*/}; echo
set -- ab ac
IFS=:
printf '[%s]' "${*#a}"; echo
unset IFS
x=${@#a}
echo "$x"
set --
printf '[%s]' "${@#a}" x; echo
