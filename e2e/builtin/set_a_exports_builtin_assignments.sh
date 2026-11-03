#!/bin/sh
# POSIX_REF: 2.15 Special Built-In Utilities - set -a
# DESCRIPTION: set -a exports variables assigned by read, getopts, for, ${x=} and $((x=)) too (dash and bash agree)
# EXPECT_OUTPUT: y=yv o=a i=fv d=dv n=5
set -a
read y <<EOF
yv
EOF
getopts a o -a
for i in fv; do :; done
: ${d=dv} $((n=5))
set +a
/bin/sh -c 'echo "y=$y o=$o i=$i d=$d n=$n"'
