#!/bin/sh
# POSIX_REF: 2.15 exec
# DESCRIPTION: exec N>&- closes fd N for the current shell; reading from closed fd is a redirection error
# EXPECT_EXIT: 1
exec 3>&-
read line 0<&3 2>/dev/null
