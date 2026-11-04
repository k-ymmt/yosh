#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -o nolog is a POSIX option name: accepted, listed, and restorable via set +o
# EXPECT_OUTPUT<<END
# set -o nolog
# set +o nolog
# END
# EXPECT_EXIT: 0
set -o nolog
saved=$(set +o)
printf '%s\n' "$saved" | grep nolog
set +o nolog
set +o | grep nolog
eval "$saved"
