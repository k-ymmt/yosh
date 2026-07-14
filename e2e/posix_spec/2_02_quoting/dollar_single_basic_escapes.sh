#!/bin/sh
# POSIX_REF: 2.2.4 Dollar-Single-Quotes
# DESCRIPTION: $'...' expands \n \t \\ \' \" to the corresponding characters
# EXPECT_OUTPUT<<END
# nl-ok
# tab-ok
# bs-ok
# sq-ok
# dq-ok
# END
# EXPECT_EXIT: 0
v=$'a\nb'
[ "$v" = "$(printf 'a\nb')" ] && echo nl-ok
v=$'a\tb'
[ "$v" = "$(printf 'a\tb')" ] && echo tab-ok
v=$'a\\b'
[ "$v" = 'a\b' ] && echo bs-ok
v=$'a\'b'
[ "$v" = "a'b" ] && echo sq-ok
v=$'a\"b'
[ "$v" = 'a"b' ] && echo dq-ok
