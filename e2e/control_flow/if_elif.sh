#!/bin/sh
# POSIX_REF: 2.9.4.1 if Conditional Construct
# DESCRIPTION: elif chain picks the first true branch
# EXPECT_OUTPUT: second
if false; then echo first; elif true; then echo second; elif true; then echo third; fi
