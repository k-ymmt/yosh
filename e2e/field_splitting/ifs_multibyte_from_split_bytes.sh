#!/bin/sh
# POSIX_REF: 2.6.5 Field Splitting
# DESCRIPTION: A multibyte IFS character splits even when its bytes come from separate command substitutions
# EXPECT_OUTPUT<<END
# <a>
# <b>
# END
# EXPECT_EXIT: 0
IFS=$(printf '\303\251')
x=$(printf 'a\303')$(printf '\251b')
for f in $x; do [ -n "$f" ] && printf '<%s>\n' "$f"; done
