#!/bin/sh
# POSIX_REF: 2.15 trap
# DESCRIPTION: SIGPIPE is trappable (not misclassified as ignored on entry)
# EXPECT_OUTPUT<<END
# p
# after
# END
# EXPECT_EXIT: 0
trap 'echo p' PIPE
kill -PIPE $$
echo after
