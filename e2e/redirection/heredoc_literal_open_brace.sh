#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A literal { inside ${...} default text needs no closer - the first } closes the expansion
# EXPECT_OUTPUT<<END
# {
# {}
# a
# END
unset x y
cat <<XEOF
${x:-{}
${x:-{}}
${x:-${y:-a}}
XEOF
