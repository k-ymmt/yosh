#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: ${var+alt} vs ${var:+alt} — empty string handling
# EXPECT_EXIT: 0
x=set
r1="${x+alt}"
unset x
r2="${x+alt}"
y=
r3="${y:+alt}"
unset z
r4="${z:+alt}"
z=set
r5="${z:+alt}"
test "$r1" = "alt" && test "$r2" = "" && test "$r3" = "" && test "$r4" = "" && test "$r5" = "alt"
