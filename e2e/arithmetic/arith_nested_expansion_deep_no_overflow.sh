#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Thousands of nested $((...)) expansions never overflow the native stack; the EXIT trap always runs
# EXPECT_OUTPUT<<END
# trapped
# end
# END
# EXPECT_EXIT: 0
# Each nested $(( )) is expanded by a recursive call before its text is
# parsed, so the arithmetic parser's own nesting limit never sees it.
# Depending on the build the expansion either succeeds (dash and bash
# print 1) or fails with an arithmetic error; either way the subshell's
# EXIT trap must run instead of the shell aborting.
s=1
i=0
while [ "$i" -lt 3000 ]; do s='$(('"$s"'))'; i=$((i+1)); done
(trap 'echo trapped' EXIT; eval ': '"$s") 2>/dev/null
echo end
