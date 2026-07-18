#!/bin/sh
# POSIX_REF: 2.15 dot
# DESCRIPTION: Use case - generate a settings file via heredoc and source it with dot
# EXPECT_OUTPUT: yosh-1.0
mkdir "$TEST_TMPDIR/work" && cd "$TEST_TMPDIR/work" || exit 1
cat > settings.sh <<EOF
NAME=yosh
VERSION=1.0
EOF
. ./settings.sh
echo "$NAME-$VERSION"
