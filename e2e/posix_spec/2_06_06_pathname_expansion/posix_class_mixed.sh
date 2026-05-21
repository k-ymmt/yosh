#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: POSIX class can be combined with literal range
# EXPECT_EXIT: 0
case 5 in [[:alpha:]0-9]) exit 0 ;; *) exit 1 ;; esac
