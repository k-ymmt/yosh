#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: read splits on a raw-byte IFS and on a multibyte IFS character as a whole, and drops NUL input bytes
# EXPECT_OUTPUT<<END
# a|b
# ab|c
# a|bxc|
# END
IFS=$(printf '\377')
printf 'a\377b\n' | { read a b; echo "$a|$b"; }
IFS=' '
printf 'a\000b c\n' | { read x y; printf '%s|%s\n' "$x" "$y"; }
# U+00E9 and U+00E8 share their lead byte; only the whole character splits.
e=$(printf '\303\251') E=$(printf '\303\250')
IFS=$e
printf 'a%sb%sc\n' "$e" "$E" | { read a b c; printf '%s|%s|%s\n' "$a" "$(printf '%s' "$b" | sed "s/$E/x/")" "$c"; }
