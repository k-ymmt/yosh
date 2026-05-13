#!/bin/sh
# POSIX_REF: 2.14.10 exec
# DESCRIPTION: exec N>&- closes fd N for the current shell
# XFAIL: read builtin not yet implemented (TODO: implement read)
# EXPECT_EXIT: 0
exec 3>&-
read line 0<&3 2>/dev/null
