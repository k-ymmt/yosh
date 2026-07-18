#!/bin/sh
# POSIX_REF: 2.15 times
# DESCRIPTION: times accepts no operands; extra args may cause a usage error or be ignored
# EXPECT_EXIT: 0
times >/dev/null 2>&1
