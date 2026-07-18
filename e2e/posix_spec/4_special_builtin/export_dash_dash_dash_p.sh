#!/bin/sh
# POSIX_REF: 2.15 export (XBD 12.2 Guideline 10)
# DESCRIPTION: export -- ends options; trailing -p is a bad identifier
# EXPECT_STDERR: export
# EXPECT_EXIT: 1
export -- -p
