#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Pipelines work when the shell's stdin or stdout is closed (pipe ends must not land on fd 0/1)
# EXPECT_OUTPUT<<END
# in
# out
# END
( exec 0<&-; echo in | cat )
( exec 1>&-; /bin/echo out | cat >&2 ) 2>&1
