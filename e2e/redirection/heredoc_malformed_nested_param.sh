#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: Malformed nested parameter expansion in a heredoc is diagnosed - command skipped with status 1, shell continues (dash: Bad substitution, exit 2)
# EXPECT_OUTPUT<<END
# status=1
# after
# END
# EXPECT_STDERR: unknown parameter operator
unset x
sh -c "echo ran" <<XEOF
${x:-${y:bad}}
XEOF
echo status=$?
echo after
