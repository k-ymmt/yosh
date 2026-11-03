#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: The end of a backquoted command body ends a here-document whose delimiter line is missing, as in dash and bash
# EXPECT_OUTPUT<<END
# [body]
# ok
# END
# EXPECT_EXIT: 0
if false; then
x=`cat <<E
body
`
fi
x=`cat <<E
body
`
echo "[$x]"
echo ok
