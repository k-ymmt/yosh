#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: 'if' in command position is recognized as reserved
# EXPECT_OUTPUT: yes
# EXPECT_EXIT: 0
if true; then echo yes; fi
