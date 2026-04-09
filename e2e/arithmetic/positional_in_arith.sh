#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Positional parameters in arithmetic expansion
# XFAIL: Phase 5 limitation — $N inside $((...)) not supported
# EXPECT_OUTPUT: 30
set -- 10 20
echo $(($1 + $2))
