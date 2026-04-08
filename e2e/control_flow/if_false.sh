#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: if with false condition produces no output
# EXPECT_OUTPUT:
if false; then echo yes; fi
