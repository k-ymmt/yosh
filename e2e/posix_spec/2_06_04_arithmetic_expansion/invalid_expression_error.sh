#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: An invalid expression writes a diagnostic and a non-interactive shell exits
# EXPECT_OUTPUT<<END
# no-continue
# exited-nonzero
# has-diagnostic
# END
# EXPECT_EXIT: 0
out=$(./target/debug/yosh -c 'echo $((1 +)); echo unreached' 2>"$TEST_TMPDIR/err")
st=$?
case $out in *unreached*) ;; *) echo no-continue ;; esac
[ "$st" -ne 0 ] && echo exited-nonzero
[ -s "$TEST_TMPDIR/err" ] && echo has-diagnostic
