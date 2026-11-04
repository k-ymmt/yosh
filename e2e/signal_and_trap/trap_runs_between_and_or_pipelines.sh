#!/bin/sh
# POSIX_REF: 2.11 Signals and Error Handling
# DESCRIPTION: A pending trap runs after the current pipeline of an AND-OR list, before the next one
# EXPECT_OUTPUT<<END
# T
# yes
# T
# end
# X
# END
# EXPECT_EXIT: 4
trap 'echo T' USR1
kill -USR1 $$ && echo yes
false || kill -USR1 $$ || echo no
echo end
trap 'echo X; exit 4' USR2
kill -USR2 $$ && echo notreached
