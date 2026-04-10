#!/bin/sh
# POSIX_REF: 2.6.3 Command Substitution
# DESCRIPTION: Nested command substitution with quoted closing paren
# EXPECT_OUTPUT: )
echo $(echo $(echo ')'))
