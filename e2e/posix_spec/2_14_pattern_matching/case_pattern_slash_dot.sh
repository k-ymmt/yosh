#!/bin/sh
# POSIX_REF: 2.14 Pattern Matching Notation
# DESCRIPTION: In non-filename contexts wildcards match / and a leading dot
# EXPECT_OUTPUT<<END
# slash-ok
# dot-ok
# END
# EXPECT_EXIT: 0
case a/c in a*) echo slash-ok ;; esac
case .foo in *foo) echo dot-ok ;; esac
