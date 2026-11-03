#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: yosh-specific: compound nesting plus word-level nesting share one stack budget, so their combination is a syntax error (exit 2), not a stack-overflow abort
# EXPECT_OUTPUT<<END
# 2
# OK
# END
# EXPECT_EXIT: 0
rep() {
    s= i=0
    while [ "$i" -lt "$2" ]; do s="$s$1"; i=$((i + 1)); done
    printf '%s' "$s"
}
src="$(rep 'if true; then ' 190)echo $(rep '${x:-' 350)OK$(rep '}' 350)$(rep '; fi' 190)"
(eval "$src") 2>/dev/null
echo "$?"
src="$(rep 'if true; then ' 20)echo $(rep '${x:-' 100)OK$(rep '}' 100)$(rep '; fi' 20)"
eval "$src"
