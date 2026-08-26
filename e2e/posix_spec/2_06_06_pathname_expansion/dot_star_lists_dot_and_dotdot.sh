#!/bin/sh
# POSIX_REF: 2.6.6 Pathname Expansion
# DESCRIPTION: .* matches the . and .. entries; .[!.]* matches neither
# EXPECT_OUTPUT<<END
# . .. .hidden
# .hidden
# END
# EXPECT_EXIT: 0
mkdir -p "$TEST_TMPDIR/dotdir"
cd "$TEST_TMPDIR/dotdir" || exit 1
: > .hidden
: > visible
echo .*
echo .[!.]*
