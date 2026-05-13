#!/bin/sh
# POSIX_REF: 8 Environment Variables - SHELL
# DESCRIPTION: SHELL is set by the shell or its parent and is preserved
# EXPECT_EXIT: 0
[ -n "${SHELL+x}" ] && exit 0
exit 1
