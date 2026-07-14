#!/bin/sh
# POSIX_REF: 2.15 set
# DESCRIPTION: set -o with no arguments lists options and their current state
# EXPECT_OUTPUT<<END
# lists-allexport
# pipefail-on
# END
# EXPECT_EXIT: 0
set -o pipefail
out=$(set -o)
case $out in *allexport*) echo lists-allexport ;; esac
line=$(set -o | grep pipefail)
case $line in *pipefail*on*) echo pipefail-on ;; esac
