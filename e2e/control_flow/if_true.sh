#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: if with true condition executes then-body
# EXPECT_OUTPUT: yes
if true; then echo yes; fi
