#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: Runaway function / eval / dot recursion ends in a shell error with the EXIT trap run, not a native stack overflow
# EXPECT_OUTPUT<<END
# fn 1
# eval 1
# dot 1
# deep ok
# top
# END
cd "$TEST_TMPDIR" || exit 1
exec 2>/dev/null
( trap 'echo fn $?' EXIT; f() { f; }; f; echo unreachable )
( trap 'echo eval $?' EXIT; e='eval "$e"'; eval "$e"; echo unreachable )
echo '. ./self.sh' >self.sh
( trap 'echo dot $?' EXIT; . ./self.sh; echo unreachable )
# Moderate recursion still works.
g() { if [ "$1" -gt 0 ]; then g $(($1 - 1)); else echo deep ok; fi; }
g 200
echo top
