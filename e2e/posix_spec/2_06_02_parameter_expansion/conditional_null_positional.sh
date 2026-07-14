#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: A positional parameter set to the empty string counts as set for ${1+word} and ${1-word}
# EXPECT_OUTPUT: [alt][]
# EXPECT_EXIT: 0
set -- ""
echo "[${1+alt}][${1-d}]"
