#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_ALL
# DESCRIPTION: LC_ALL overrides all other LC_* and LANG
# EXPECT_EXIT: 0
LC_ALL=C
LANG=en_US.UTF-8
# When LC_ALL is set, LANG should not affect output
[ "$LC_ALL" = C ] || exit 1
