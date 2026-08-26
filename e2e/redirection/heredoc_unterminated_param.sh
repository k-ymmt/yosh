#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Unterminated ${ in a heredoc body is diagnosed as a redirection error - command skipped with status 1, shell continues
# EXPECT_OUTPUT<<END
# status=1
# after
# END
# EXPECT_STDERR: unterminated parameter expansion
cat <<XEOF
${x
XEOF
echo status=$?
echo after
