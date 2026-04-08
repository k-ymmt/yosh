#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Assign default with := when unset, variable persists
# EXPECT_OUTPUT<<END
# assigned
# assigned
# END
unset x
echo "${x:=assigned}"
echo "$x"
