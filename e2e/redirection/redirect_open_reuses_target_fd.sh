#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: A redirection onto a closed fd does not leak the opened file past the command
# EXPECT_OUTPUT<<END
# not leaked
# g is empty
# END
cd "$TEST_TMPDIR" || exit 1
{ :; } 3>f
{ echo LEAKED >&3; } 2>/dev/null || echo not leaked
true 3>g
/bin/sh -c 'echo child >&3' 2>/dev/null
[ -s g ] || echo g is empty
