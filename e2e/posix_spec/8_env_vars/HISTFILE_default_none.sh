#!/bin/sh
# POSIX_REF: 8 Environment Variables - HISTFILE
# DESCRIPTION: if HISTFILE is unset, no history is saved
# EXPECT_EXIT: 0
unset HISTFILE
exit 0
