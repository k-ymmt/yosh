#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: A non-ASCII IFS character, or a raw non-UTF-8 byte, splits fields as a whole character
# EXPECT_OUTPUT<<END
# 3 [a][b][c]
# 2 [a][b]
# 1 [aéb]
# END
# EXPECT_EXIT: 0
IFS=é
x=aébéc
set -- $x
printf '%s ' $#; printf '[%s]' "$@"; echo
IFS=$(printf '\377')
x=$(printf 'a\377b')
set -- $x
printf '%s ' $#; printf '[%s]' "$@"; echo
IFS=é
set -- "aéb"
printf '%s ' $#; printf '[%s]' "$@"; echo
