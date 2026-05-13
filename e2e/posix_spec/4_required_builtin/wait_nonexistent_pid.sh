#!/bin/sh
# POSIX_REF: 4 Utilities - wait
# DESCRIPTION: wait of a pid that is not a child returns 127
# EXPECT_EXIT: 127
wait 99999 2>/dev/null
