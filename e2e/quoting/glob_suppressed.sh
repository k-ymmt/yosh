#!/bin/sh
# POSIX_REF: 2.2 Quoting
# DESCRIPTION: Quoting suppresses glob expansion
# EXPECT_OUTPUT<<END
# src/*.rs
# src/*.rs
# END
echo 'src/*.rs'
echo "src/*.rs"
