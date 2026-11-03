#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: An exec failure other than not-found (E2BIG) exits 126 and is reported from a child, so a closed stderr pipe does not kill the shell and a CHLD trap fires
# EXPECT_OUTPUT<<END
# status:141
# done
# CHLD
# after:126
# END
# EXPECT_EXIT: 0
a=x; i=0
while [ "$i" -lt 23 ]; do a=$a$a; i=$((i+1)); done
exec 3>&1
{
    /usr/bin/true "$a"
    echo status:$?
    echo done
} 2>&1 1>&3 | /usr/bin/true
trap 'echo CHLD' CHLD
/usr/bin/true "$a" 2>/dev/null
echo after:$?
