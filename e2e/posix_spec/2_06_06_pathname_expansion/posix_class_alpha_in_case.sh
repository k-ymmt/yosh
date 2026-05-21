#!/bin/sh
# POSIX_REF: 9.3.5 RE Bracket Expression - Character Classes
# DESCRIPTION: [[:alpha:]] matches alphabetic in case pattern
# EXPECT_EXIT: 0
case A in [[:alpha:]]) exit 0 ;; *) exit 1 ;; esac
