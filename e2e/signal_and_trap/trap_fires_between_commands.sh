#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: A trapped signal's action runs after the current command, not after the whole list
# EXPECT_OUTPUT<<END
# T
# one
# two
# three
# END
# EXPECT_EXIT: 0
trap 'echo T' USR1
kill -USR1 $$; echo one; echo two; echo three
