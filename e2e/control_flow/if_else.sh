#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: if-else executes else-body when condition is false
# EXPECT_OUTPUT: no
if false; then echo yes; else echo no; fi
