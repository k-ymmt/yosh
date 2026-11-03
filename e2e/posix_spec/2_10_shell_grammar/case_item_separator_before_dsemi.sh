#!/bin/sh
# POSIX_REF: 2.10 Shell Grammar
# DESCRIPTION: A case item body may end with a separator before ';;' or ';&' (compound_list allows a trailing separator)
# EXPECT_OUTPUT<<END
# yes
# z
# fall
# next
# END
# EXPECT_EXIT: 0
case a in a) echo yes; ;; esac; echo z
case c in c) echo fall; ;& d) echo next; ;; esac
