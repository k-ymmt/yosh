#!/bin/sh
# POSIX_REF: 2.15 export
# DESCRIPTION: export/readonly of an unset name set the attribute but leave the variable unset
# EXPECT_OUTPUT<<END
# y=unset
# export y
# not in env
# z=unset
# readonly z
# exported after assignment: 1
# END
unset y z
export y
echo "y=${y-unset}"
export -p | grep '^export y$'
/usr/bin/env | grep -q '^y=' || echo "not in env"
readonly z
echo "z=${z-unset}"
readonly -p | grep '^readonly z$'
y=1
echo "exported after assignment: $(/bin/sh -c 'echo $y')"
