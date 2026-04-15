#!/bin/sh
# POSIX_REF: 2.6.4 Arithmetic Expansion
# DESCRIPTION: Quoted ')' in $(cmd) must not break $((...)) boundary detection
# EXPECT_OUTPUT<<END
# 4
# 4
# END
echo $(( $(echo '3)' | cut -c1) + 1 ))
echo $(( $(echo "3)" | cut -c1) + 1 ))
