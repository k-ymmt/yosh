#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: Signals ignored at non-interactive shell entry cannot be trapped
# EXPECT_OUTPUT: after
# EXPECT_EXIT: 0
perl -e '$SIG{USR1}="IGNORE"; exec @ARGV' -- ./target/debug/yosh -c 'trap "echo got" USR1; kill -USR1 $$; echo after'
