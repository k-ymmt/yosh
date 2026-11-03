#!/bin/sh
# POSIX_REF: 2.3.1 Alias Substitution
# DESCRIPTION: A literal esac pattern alternative does not end the case context used for alias substitution
# EXPECT_OUTPUT<<END
# OK
# OK
# END
# EXPECT_EXIT: 0
alias e='echo OK'
case case in
  foo|esac) :;;
  (esac) :;;
  case) :;;
esac
e
case x in (esac|x) e;; esac
