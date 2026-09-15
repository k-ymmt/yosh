#!/bin/sh
# POSIX_REF: 4 Utilities - printf
# DESCRIPTION: printf is a builtin: conversions, escapes, and format reuse
# EXPECT_OUTPUT<<END
# a|b
# c|
# 5 -7 3 31 8
# 18446744073709551615 ffffffffffffffff 1777777777777777777777
#    he|ab   |00042|+42| 42
# 3.142 1.234500e+03 0.0001234 1E+20 0xff 010
# h|    x|
# A	tab
# builtin
# END
printf '%s|%s\n' a b c
printf '%d %i %d %d %d\n' 5 -7 +3 0x1f 010
printf '%u %x %o\n' -1 -1 -1
printf '%5.2s|%-5s|%05d|%+d|% d\n' hello ab 42 42 42
printf '%.3f %e %g %G %#x %#o\n' 3.14159 1234.5 0.0001234 1e20 255 8
printf '%c%c|%5c|\n' hello '' x
printf '\101\ttab\n'
type printf | sed 's/.*is a //; s/.*is //' | grep -q builtin && echo builtin
