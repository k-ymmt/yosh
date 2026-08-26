#!/bin/sh
# POSIX_REF: 2.7.4 Here-Document
# DESCRIPTION: set -u unset variable in a heredoc body aborts the command (not the shell), same as ${x:?}
# EXPECT_OUTPUT<<END
# status=1
# after
# END
# EXPECT_STDERR: parameter not set
set -u
unset x
sh -c "echo ran" <<XEOF
$x
XEOF
echo status=$?
echo after
