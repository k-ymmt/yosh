#!/bin/sh
# POSIX_REF: 2.13 Pattern Matching Notation
# DESCRIPTION: '?' matches exactly one character
# EXPECT_OUTPUT<<END
# one
# notone
# END
# EXPECT_EXIT: 0
case a in ?) echo one ;; *) echo notone ;; esac
case ab in ?) echo one ;; *) echo notone ;; esac
