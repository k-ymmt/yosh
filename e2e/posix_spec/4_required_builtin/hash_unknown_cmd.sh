#!/bin/sh
# POSIX_REF: 4 Utilities - hash
# DESCRIPTION: hash of a nonexistent utility is an error
# XFAIL: non-POSIX deviation (yosh has no native hash builtin; falls through to /usr/bin/hash which does not error on unknown commands)
# EXPECT_EXIT: 1
hash /no/such/cmd_$$ 2>/dev/null
