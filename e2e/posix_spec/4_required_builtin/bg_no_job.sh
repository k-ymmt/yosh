#!/bin/sh
# POSIX_REF: 4 Utilities - bg
# DESCRIPTION: bg with no current job is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: bg
set -m 2>/dev/null
bg %1 >/dev/null
