#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: ${x:?msg} failure in a heredoc body aborts the command (not the shell) - matches dash
# EXPECT_OUTPUT<<END
# status=1
# after
# END
# EXPECT_STDERR: boom
unset x
sh -c "echo ran" <<XEOF
${x:?boom}
XEOF
echo status=$?
echo after
