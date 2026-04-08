#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Deeply nested command substitution
# EXPECT_OUTPUT: deep
echo $(echo $(echo $(echo deep)))
