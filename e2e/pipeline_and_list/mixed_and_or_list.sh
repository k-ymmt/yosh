#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Mixed && and || evaluate left to right
# EXPECT_OUTPUT<<END
# recovered
# final
# END
false && echo no || echo recovered
true && echo final || echo no
