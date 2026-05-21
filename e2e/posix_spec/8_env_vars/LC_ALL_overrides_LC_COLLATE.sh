#!/bin/sh
# POSIX_REF: 8 Environment Variables - LC_ALL
# DESCRIPTION: LC_ALL overrides LC_COLLATE; internal pattern uses C semantics
# EXPECT_EXIT: 0
LC_ALL=C
LC_COLLATE=fr_FR.UTF-8
# Under C semantics, [A-Z] matches uppercase ASCII only.
case M in [A-Z]) exit 0 ;; *) exit 1 ;; esac
