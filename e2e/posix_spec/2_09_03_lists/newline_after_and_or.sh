#!/bin/sh
# POSIX_REF: 2.9.3 Lists
# DESCRIPTION: A newline after && or || continues the list
# EXPECT_OUTPUT<<END
# and-continued
# or-continued
# END
# EXPECT_EXIT: 0
true &&
echo and-continued
false ||
echo or-continued
