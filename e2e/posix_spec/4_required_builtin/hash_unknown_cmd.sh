#!/bin/sh
# POSIX_REF: 4 Utilities - hash
# DESCRIPTION: hash of a nonexistent utility is an error
# XFAIL: hash does not return nonzero for unknown commands (via /usr/bin/hash wrapper)
# EXPECT_EXIT: 1
hash /no/such/cmd_$$ 2>/dev/null
