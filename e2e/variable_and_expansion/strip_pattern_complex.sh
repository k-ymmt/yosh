#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: Complex glob pattern in prefix/suffix stripping
# EXPECT_OUTPUT<<END
# /home/user
# document.tar
# END
path="/home/user/documents"
echo "${path%/*}"
file="document.tar.gz"
echo "${file%.*}"
