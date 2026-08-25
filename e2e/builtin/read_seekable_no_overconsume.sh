#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read from a seekable file consumes exactly one line; the fd offset is left just past the newline for the next reader
# EXPECT_OUTPUT<<END
# first=line1
# line2
# line3
# END
# EXPECT_EXIT: 0
tmp="${TMPDIR:-/tmp}/yosh_read_seek_$$"
printf 'line1\nline2\nline3\n' > "$tmp"
{
    read a
    echo "first=$a"
    cat
} < "$tmp"
rm -f "$tmp"
