#!/bin/sh
# POSIX_REF: 2.7 Redirection
# DESCRIPTION: Redirecting an fd that is currently closed works, and the fd is closed again afterwards
# EXPECT_OUTPUT<<END
# x
# rc=0
# hi
# after
# closed again
# END
{ echo x >&5; } 5>&1
echo rc=$?
echo hi 5>/dev/null
: 3>&-
echo after
{ echo leak >&5; } 2>/dev/null || echo closed again
