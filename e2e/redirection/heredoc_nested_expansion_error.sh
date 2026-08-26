#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: A failing nested expansion inside ${x:-...} in a heredoc aborts the command (not the shell) - matches bash/dash
# EXPECT_OUTPUT<<END
# status=1
# after
# END
# EXPECT_STDERR: division by zero
unset x
sh -c "echo ran" <<XEOF
${x:-$((1/0))}
XEOF
echo status=$?
echo after
