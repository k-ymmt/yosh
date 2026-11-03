#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: umask -S with an operand sets the mask; -- ends options; unknown options are errors
# EXPECT_OUTPUT<<END
# 0077
# 0027
# u=rwx,g=rx,o=
# 2
# 0027
# END
# EXPECT_EXIT: 0
umask -S 077
umask
umask -- 027
umask
umask -S
umask -x 2>/dev/null; echo $?
umask
