#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: An expansion error in PS4 is diagnosed, PS4 is shown literally, and the shell continues
# EXPECT_OUTPUT<<END
# ${u?boom} echo a
# a
# ${u?boom} echo after
# after
# diagnosed
# status=0
# END
# EXPECT_EXIT: 0
out=$( (PS4='${u?boom} '; set -x; echo a; echo after) 2>&1 )
st=$?
printf '%s\n' "$out" | grep -v 'boom$'
case $out in *'u: boom'*) echo diagnosed ;; esac
echo "status=$st"
