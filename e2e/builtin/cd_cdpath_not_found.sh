#!/bin/sh
# POSIX_REF: 4 Utilities - cd
# DESCRIPTION: When CDPATH has no match, cd falls back to normal resolution and errors if the operand does not exist
# EXPECT_EXIT: 1
CDPATH=/tmp cd nonexistent_xyz_zzz
