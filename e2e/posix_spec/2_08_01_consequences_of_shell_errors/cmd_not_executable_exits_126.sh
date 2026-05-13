#!/bin/sh
# POSIX_REF: 2.8.2 Exit Status for Commands
# DESCRIPTION: Command exists but is not executable yields exit status 126
# EXPECT_OUTPUT: 126
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR"
: > notexec
chmod 644 notexec
./notexec 2>/dev/null
echo $?
