#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: Use case - guard commands with && and provide fallbacks with ||
# EXPECT_OUTPUT<<END
# created
# exists
# missing
# END
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
mkdir newdir && echo created
[ -d newdir ] && echo exists || echo absent
[ -d nonexistent ] && echo exists || echo missing
