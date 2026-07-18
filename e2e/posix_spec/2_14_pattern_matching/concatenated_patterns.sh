#!/bin/sh
# POSIX_REF: 2.14 Pattern Matching Notation
# DESCRIPTION: Concatenated single- and multi-character patterns match as a unit
# EXPECT_OUTPUT<<END
# match
# no-match
# END
# EXPECT_EXIT: 0
case abcxd in a?c*d) echo match ;; *) echo no-match ;; esac
case axd in a?c*d) echo match ;; *) echo no-match ;; esac
