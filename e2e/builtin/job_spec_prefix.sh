#!/bin/sh
# POSIX_REF: 3.204 Job Control Job ID
# DESCRIPTION: %string prefix match resolves a unique background job
# EXPECT_EXIT: 0
sleep 0.1 &
wait %sleep
