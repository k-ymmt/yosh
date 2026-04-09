#!/bin/sh
# POSIX_REF: 2.12 Shell Execution Environment
# DESCRIPTION: Exit in subshell does not terminate parent
# EXPECT_OUTPUT<<END
# sub-exiting
# parent-alive
# END
(echo sub-exiting; exit 1)
echo parent-alive
