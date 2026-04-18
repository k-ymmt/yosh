#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Quoting a reserved word in command position looks up as command, not reserved
# EXPECT_EXIT: 127
# EXPECT_STDERR: not found
'if' true
