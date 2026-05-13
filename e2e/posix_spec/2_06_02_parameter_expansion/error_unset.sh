#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var:?msg} causes shell error when var is unset (non-interactive: exit)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
# EXPECT_STDERR: missing
(unset x; echo "${x:?missing}")
