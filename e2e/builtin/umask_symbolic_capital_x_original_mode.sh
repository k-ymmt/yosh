#!/bin/sh
# POSIX_REF: 4 Utilities - umask
# DESCRIPTION: X in a symbolic umask tests the original mode, not the mode modified by earlier actions
# EXPECT_OUTPUT<<END
# 0000
# 0000
# 0011
# END
# EXPECT_EXIT: 0
umask 000; umask a-x+X; umask
umask 000; umask a-x,a+X; umask
umask 111; umask u+x,g+X; umask
