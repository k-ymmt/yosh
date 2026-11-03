#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: A fatal expansion error in an EXIT trap run by a subshell or command substitution makes that child exit non-zero (set -e then stops the parent)
# EXPECT_OUTPUT<<END
# cmdsub-nonzero out=in
# subshell-nonzero
# exit-nonzero
# errexit-cmdsub-stopped
# errexit-subshell-stopped
# END
# EXPECT_EXIT: 0
exec 2>/dev/null
unset x
out=$(trap ': ${x:?missing}' EXIT; echo in)
[ $? -ne 0 ] && echo "cmdsub-nonzero out=$out"
(trap ': ${x:?missing}' EXIT; true)
[ $? -ne 0 ] && echo subshell-nonzero
(trap ': ${x:?missing}' EXIT; exit 0)
[ $? -ne 0 ] && echo exit-nonzero
r=$(set -e; out=$(trap ': ${x:?missing}' EXIT; :); echo survived)
[ -z "$r" ] && echo errexit-cmdsub-stopped
r=$(set -e; (trap ': ${x:?missing}' EXIT; true); echo survived)
[ -z "$r" ] && echo errexit-subshell-stopped
