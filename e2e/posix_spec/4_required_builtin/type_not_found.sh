#!/bin/sh
# POSIX_REF: 4 Utilities - type
# DESCRIPTION: type on a nonexistent name exits nonzero
# EXPECT_EXIT: 1
type /no/such/cmd_$$ 2>/dev/null
