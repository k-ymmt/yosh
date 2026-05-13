#!/bin/sh
# POSIX_REF: 2.14.17 trap
# DESCRIPTION: trap runs handler then resumes
# XFAIL: non-POSIX deviation (yosh defers INT trap to end-of-script; handler runs after 'after')
# EXPECT_OUTPUT<<END
# caught
# after
# END
# EXPECT_EXIT: 0
trap 'echo caught' INT
kill -INT $$ 2>/dev/null
sleep 0.05 2>/dev/null
echo after
