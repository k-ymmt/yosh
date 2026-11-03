#!/bin/sh
# POSIX_REF: 2.10.1 Shell Grammar Lexical Conventions
# DESCRIPTION: Pathologically deep ${...}/$(...) nesting is a syntax error, not a stack overflow; moderate nesting works
# EXPECT_OUTPUT<<END
# z
# z
# 2
# 2
# END
gen() {
    i=0 open= close=
    while [ $i -lt $1 ]; do
        open="$open$2" close="$close$3"
        i=$((i + 1))
    done
    printf 'echo %sz%s\n' "$open" "$close"
}
(eval "$(gen 50 '${x:-' '}')")
(eval "$(gen 20 '$(echo ' ')')")
(eval "$(gen 5000 '${x:-' '}')") 2>/dev/null; echo $?
(eval "$(gen 2000 '$(echo ' ')')") 2>/dev/null; echo $?
