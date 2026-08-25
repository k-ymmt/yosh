#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Special parameters with modifiers resolve as set (not unset)
# EXPECT_OUTPUT<<END
# 0
# empty
# flags
# 0
# none
# a,b
# END
echo "${?:-x}"
echo "${!:-empty}"
echo "${-+flags}"
echo "${#:-x}"
set --
echo "${*:-none}"
set -- a b
IFS=,
echo "${*:-none}"
