#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_COLLATE
# DESCRIPTION: LC_COLLATE affects range collation in patterns
# EXPECT_EXIT: 0
LC_COLLATE=C
case M in [A-Z]) exit 0 ;; *) exit 1 ;; esac
