#!/bin/sh
# POSIX_REF: 4 Utilities - bg
# DESCRIPTION: bg with malformed job spec is an error
# EXPECT_EXIT: 1
# EXPECT_STDERR: bg
bg %notajob >/dev/null
