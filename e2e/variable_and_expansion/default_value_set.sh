#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Default value not used when variable is set
# EXPECT_OUTPUT: actual
x=actual
echo "${x:-fallback}"
