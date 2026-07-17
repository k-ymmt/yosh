#!/bin/sh
# POSIX_REF: 2.9.2 Pipelines
# DESCRIPTION: Use case - count matching log lines by piping into grep -c
# EXPECT_OUTPUT: 2
cat <<EOF | grep -c ERROR
INFO start
ERROR one
WARN mid
ERROR two
INFO end
EOF
