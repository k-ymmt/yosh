#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Inside double quotes, backslash-dquote in a backquoted command loses its backslash; outside it is kept
# EXPECT_OUTPUT<<END
# hi
# "hi"
# hi
# END
echo "`echo \"hi\"`"
echo `echo \"hi\"`
unset x
echo "${x-`echo \"hi\"`}"
