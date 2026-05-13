#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var?msg} errors only on unset (no colon = unset-only check)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: missing
(unset x; echo "${x?missing}")
