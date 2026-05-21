#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: [![:digit:]] matches non-digit (negation of POSIX class)
# EXPECT_EXIT: 0
case a in [![:digit:]]) exit 0 ;; *) exit 1 ;; esac
