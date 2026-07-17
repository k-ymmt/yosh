#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Use case - fall back to defaults for unset configuration variables
# EXPECT_OUTPUT<<END
# port=8080
# port=9090
# retries=3
# END
unset PORT
echo "port=${PORT:-8080}"
PORT=9090
echo "port=${PORT:-8080}"
unset RETRIES
: "${RETRIES:=3}"
echo "retries=$RETRIES"
