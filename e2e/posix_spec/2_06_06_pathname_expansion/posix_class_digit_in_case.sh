#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: [[:digit:]] matches digit in case pattern
# EXPECT_EXIT: 0
case 5 in [[:digit:]]) exit 0 ;; *) exit 1 ;; esac
