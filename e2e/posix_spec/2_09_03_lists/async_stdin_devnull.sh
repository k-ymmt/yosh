#!/bin/sh
# POSIX_REF: 2.9.3.1 Asynchronous Lists
# DESCRIPTION: With job control disabled, stdin of an async command is /dev/null
# EXPECT_OUTPUT<<END
# st=1
# y=data
# END
# EXPECT_EXIT: 0
# XFAIL: yosh lets async commands read the shell's stdin instead of /dev/null
printf 'data\n' > "$TEST_TMPDIR/in"
./target/debug/yosh -c 'read x & wait $!; echo "st=$?"; read y; echo "y=$y"' < "$TEST_TMPDIR/in"
