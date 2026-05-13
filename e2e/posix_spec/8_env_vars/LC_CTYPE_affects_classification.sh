#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_CTYPE
# DESCRIPTION: LC_CTYPE affects character classification (toupper/tolower behavior in case patterns)
# EXPECT_EXIT: 0
LC_CTYPE=C
case A in [a-z]) exit 1 ;; *) exit 0 ;; esac
