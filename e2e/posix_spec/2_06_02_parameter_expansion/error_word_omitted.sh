#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:?} with word omitted writes a default diagnostic and exits the shell
# EXPECT_OUTPUT<<END
# no-stdout
# exited-nonzero
# has-diagnostic
# END
# EXPECT_EXIT: 0
out=$(./target/debug/yosh -c 'echo ${unsetvar:?}; echo unreached' 2>"$TEST_TMPDIR/err")
st=$?
[ -z "$out" ] && echo no-stdout
[ "$st" -ne 0 ] && echo exited-nonzero
grep -q unsetvar "$TEST_TMPDIR/err" && echo has-diagnostic
