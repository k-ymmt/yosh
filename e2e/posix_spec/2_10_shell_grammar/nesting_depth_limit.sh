#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: yosh-specific: absurdly deep nesting is a syntax error (exit 2), not a stack-overflow abort; moderate nesting works
# EXPECT_OUTPUT<<END
# 2
# 2
# d
# END
# EXPECT_EXIT: 0
deep() {
    s=$3 i=0
    while [ "$i" -lt "$4" ]; do s="$1$s$2"; i=$((i + 1)); done
    printf '%s\n' "$s"
}
(eval "$(deep '(' ')' 'echo d' 500)") 2>/dev/null
echo "$?"
(eval "if false; then $(deep 'if true; then ' '; fi' 'echo d' 500); fi") 2>/dev/null
echo "$?"
eval "$(deep '{ ' '; }' 'echo d' 100)"
