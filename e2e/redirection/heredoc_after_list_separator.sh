#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Here-document body follows the next newline even after ; && or another command
# EXPECT_OUTPUT<<END
# body1
# hi
# body2
# hi
# one
# two
# END
cat <<EOF; echo hi
body1
EOF
cat <<EOF && echo hi
body2
EOF
cat <<A; cat <<B
one
A
two
B
