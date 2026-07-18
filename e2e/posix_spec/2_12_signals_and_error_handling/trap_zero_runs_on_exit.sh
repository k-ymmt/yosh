#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: trap on signal 0 / EXIT runs at shell exit
# EXPECT_OUTPUT: bye
# EXPECT_EXIT: 0
(trap 'echo bye' 0; :)
