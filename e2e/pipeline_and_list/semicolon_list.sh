#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Semicolons separate sequential commands
# EXPECT_OUTPUT<<END
# first
# second
# third
# END
echo first; echo second; echo third
