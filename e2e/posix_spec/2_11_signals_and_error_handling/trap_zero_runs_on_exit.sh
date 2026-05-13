#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: trap on signal 0 / EXIT runs at shell exit
# XFAIL: not yet implemented (TODO: trap 0/EXIT not fired on subshell exit)
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
(trap 'echo bye' 0; :)
