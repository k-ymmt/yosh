#!/bin/sh
# POSIX_REF: 4 Utilities - fc
# DESCRIPTION: fc is a regular built-in - a function may shadow it and prefix assignments do not persist
# EXPECT_OUTPUT<<END
# func
# x=
# fc is a shell builtin
# END
# EXPECT_EXIT: 0
fc() { echo func; }
fc
unset -f fc
x=1 fc -l >/dev/null 2>&1
echo "x=$x"
command -V fc
