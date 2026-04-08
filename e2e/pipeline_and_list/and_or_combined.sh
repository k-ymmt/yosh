#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Combined AND/OR list
# EXPECT_OUTPUT: ok
false || true && echo ok
