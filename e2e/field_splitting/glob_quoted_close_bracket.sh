#!/bin/sh
# POSIX_REF: 2.13.1 Patterns Matching a Single Character
# DESCRIPTION: A quoted ] cannot close an unquoted [ - the bracket stays literal and no glob occurs
# EXPECT_OUTPUT<<END
# <sr[c]>
# <sr[c]>
# <src>
# END
cd "$TEST_TMPDIR"
: > src
printf "<%s>\n" sr[c"]"
printf "<%s>\n" sr["c]"
printf "<%s>\n" sr[c]
