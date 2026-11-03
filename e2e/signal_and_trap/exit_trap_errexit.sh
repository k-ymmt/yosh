#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: set -e applies inside the EXIT trap action (dash and bash agree)
# EXPECT_OUTPUT:
# EXPECT_EXIT: 1
set -e
trap 'false; echo intrap' EXIT
true
