#!/bin/sh
# POSIX_REF: 2.14 Special Built-In Utilities
# DESCRIPTION: set -m / set +m toggles m flag in $-
# EXPECT_OUTPUT<<END
# has_m=no
# has_m=yes
# has_m=no
# END
# EXPECT_EXIT: 0

# Scripts start with monitor off
case "$-" in *m*) echo "has_m=yes" ;; *) echo "has_m=no" ;; esac

set -m
case "$-" in *m*) echo "has_m=yes" ;; *) echo "has_m=no" ;; esac

set +m
case "$-" in *m*) echo "has_m=yes" ;; *) echo "has_m=no" ;; esac
