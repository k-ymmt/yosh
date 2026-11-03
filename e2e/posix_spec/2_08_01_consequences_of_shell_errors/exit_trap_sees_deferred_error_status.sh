#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: The EXIT trap sees the fatal error's status as $?, not the status of a surrounding conditional, and is not cancelled by a fatal expansion error
# EXPECT_OUTPUT<<END
# 1
# trap:1
# 1
# END
# EXPECT_EXIT: 0
(trap 'exit' EXIT; readonly r=1; if r=2 /usr/bin/true; then :; fi) 2>/dev/null
echo "$?"
(trap 'echo trap:$?' EXIT; unset x; if : ${x:?}; then :; fi) 2>/dev/null
echo "$?"
