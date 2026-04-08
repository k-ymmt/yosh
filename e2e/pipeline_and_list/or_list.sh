#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: OR list - second command runs only if first fails
# EXPECT_OUTPUT: fallback
false || echo fallback
true || echo no
