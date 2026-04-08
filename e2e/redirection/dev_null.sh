#!/bin/sh
# POSIX_REF: 2.7.2 Redirecting Output
# DESCRIPTION: Redirect to /dev/null discards output
# EXPECT_OUTPUT:
echo hidden > /dev/null
echo ""
