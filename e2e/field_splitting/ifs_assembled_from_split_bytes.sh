#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: An IFS whose multibyte character is assembled from separate command substitutions is still that one character
# EXPECT_OUTPUT<<END
# <a>
# <b>
# <c>
# <d>
# END
# EXPECT_EXIT: 0
IFS=$(printf '\303')$(printf '\251')
x=aéb
for f in $x; do printf '<%s>\n' "$f"; done
y=$(printf 'c\303')$(printf '\251d')
for f in $y; do printf '<%s>\n' "$f"; done
