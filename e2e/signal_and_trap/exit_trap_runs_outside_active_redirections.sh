#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: The EXIT trap of a shell exiting from inside redirected commands writes to the shell's own fds (dash and bash unwind the redirections first)
# EXPECT_OUTPUT<<END
# exit 3
# fn 4
# errexit 1
# arith 1
# case 1
# loop 2
# sub
# f1 empty
# END
cd "$TEST_TMPDIR" || exit 1
exec 2>/dev/null
( trap 'echo exit $?' EXIT; { exit 3; } >f1 )
( trap 'echo fn $?' EXIT; f() { exit 4; }; f >f1 )
( set -e; trap 'echo errexit $?' EXIT; { false; } >f1 )
( trap 'echo arith $?' EXIT; for i in $((1/0)); do :; done >f1 )
( trap 'echo case $?' EXIT; case $((1/0)) in *) ;; esac >f1 )
( trap 'echo loop $?' EXIT; for i in a; do exit 2; done >f1 )
# A subshell's own redirections are part of its environment.
( trap 'echo sub' EXIT; exit 3 ) >f2
cat f2
[ -s f1 ] || echo f1 empty
