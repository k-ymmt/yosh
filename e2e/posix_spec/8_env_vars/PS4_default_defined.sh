#!/bin/sh
# POSIX_REF: 8 Environment Variables - PS4
# DESCRIPTION: PS4 is defined as a shell variable with its default value at startup
# EXPECT_OUTPUT: [+ ]
# EXPECT_EXIT: 0
echo "[${PS4-unset}]"
