#!/bin/sh
# POSIX_REF: 2.10.2 Shell Grammar Rules
# DESCRIPTION: A reserved word or ) closing the enclosing construct may follow a compound command directly
# EXPECT_OUTPUT<<END
# ok1
# ok2
# ok3
# ok4
# END
# EXPECT_EXIT: 0
if (true) then echo ok1; fi
{ (echo ok2) }
case x in x) (echo ok3) esac
(while (false) do :; done; echo ok4)
