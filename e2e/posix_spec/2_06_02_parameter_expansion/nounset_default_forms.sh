#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Under set -u, ${unset-word} and ${unset:-word} do not error
# EXPECT_OUTPUT: f g
# EXPECT_EXIT: 0
set -u
echo "${u1z-f} ${u2z:-g}"
