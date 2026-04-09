#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var+alt} vs ${var:+alt} — empty string handling
# EXPECT_OUTPUT<<END
# 
# alt
# 
# 
# alt
# END
unset x
echo "${x+alt}"
x=set
echo "${x+alt}"
y=
echo "${y:+alt}"
unset z
echo "${z:+alt}"
z=set
echo "${z:+alt}"
