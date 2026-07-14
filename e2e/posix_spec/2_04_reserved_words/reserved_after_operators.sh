#!/bin/sh
# POSIX_REF: 2.4 Reserved Words
# DESCRIPTION: Reserved words are recognized after && || ; and (
# EXPECT_OUTPUT<<END
# and-ok
# or-ok
# semi-ok
# paren-ok
# END
# EXPECT_EXIT: 0
true && if true; then echo and-ok; fi
false || if true; then echo or-ok; fi
:; if true; then echo semi-ok; fi
( if true; then echo paren-ok; fi )
