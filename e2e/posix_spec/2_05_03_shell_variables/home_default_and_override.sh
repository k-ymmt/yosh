#!/bin/sh
# POSIX_REF: 2.5.3 Shell Variables
# DESCRIPTION: HOME can be overridden and read back
# EXPECT_OUTPUT: /tmp/h
# EXPECT_EXIT: 0
HOME=/tmp/h
echo "$HOME"
