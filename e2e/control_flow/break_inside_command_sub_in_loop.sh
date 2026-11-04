#!/bin/sh
# POSIX_REF: 2.15 Special Built-In Utilities - break
# DESCRIPTION: break/continue inside $( ) within a loop end the substitution's own commands
# EXPECT_OUTPUT<<END
# i=1 []
# i=2 []
# [z]
# END
# EXPECT_EXIT: 0
for i in 1 2; do x=$(break; echo x); echo "i=$i [$x]"; done
while :; do x=$(continue; echo y)$(echo z); echo "[$x]"; break; done
