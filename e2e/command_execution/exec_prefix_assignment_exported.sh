#!/bin/sh
# POSIX_REF: 2.15 exec
# DESCRIPTION: Prefix assignments on exec with a command reach the exec'd program's environment
# EXPECT_OUTPUT: 1
x=1 exec /bin/sh -c 'echo $x'
