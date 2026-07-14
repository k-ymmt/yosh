#!/bin/sh
# POSIX_REF: 2.15 break / continue
# DESCRIPTION: break/continue inside a function do not affect the caller's loop
# EXPECT_OUTPUT<<END
# i=1
# i=2
# j=1
# j=2
# done
# END
# EXPECT_EXIT: 0
f() { break; }
for i in 1 2; do f; echo "i=$i"; done
g() { continue; }
for j in 1 2; do g; echo "j=$j"; done
echo done
