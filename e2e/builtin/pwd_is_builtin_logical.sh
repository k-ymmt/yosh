#!/bin/sh
# POSIX_REF: 4 Utilities - pwd
# DESCRIPTION: pwd is a builtin that prints the logical directory kept by cd, with -P for the physical one, independent of PATH
# EXPECT_OUTPUT<<END
# pwd is a shell builtin
# logical ok
# physical ok
# last option wins ok
# no PATH ok
# write error 1
# END
cd "$TEST_TMPDIR" || exit 1
mkdir real
ln -s real link
real=$(cd -P real && pwd -P)
type pwd
cd link || exit 1
case $(pwd) in
*/link) [ "$(pwd)" = "$PWD" ] && echo logical ok ;;
esac
[ "$(pwd -P)" = "$real" ] && echo physical ok
[ "$(pwd -PL)" = "$(pwd -L)" ] && [ "$(pwd -LP)" = "$real" ] && echo last option wins ok
# PWD is not exported here: an external pwd could not know the logical path.
[ "$(PATH=/nonexistent; pwd)" = "$(pwd -L)" ] && echo no PATH ok
pwd >&- 2>/dev/null
echo "write error $?"
