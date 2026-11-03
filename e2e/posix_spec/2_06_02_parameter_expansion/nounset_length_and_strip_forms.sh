#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion
# DESCRIPTION: set -u makes ${#x} and the strip forms of an unset variable an error; $@/$* stay exempt
# EXPECT_OUTPUT<<END
# 0
# len:err
# strip:err
# strip:err
# strip:err
# strip:err
# 1 b
# END
# EXPECT_EXIT: 0
set -u
set --
echo ${#@}
(echo ${#x}; echo unreached) 2>/dev/null || echo "len:err"
(echo ${x%a}; echo unreached) 2>/dev/null || echo "strip:err"
(echo ${x%%a}; echo unreached) 2>/dev/null || echo "strip:err"
(echo ${x#a}; echo unreached) 2>/dev/null || echo "strip:err"
(echo ${x##a}; echo unreached) 2>/dev/null || echo "strip:err"
x=ab
set -- a
echo ${#1} ${x#a}
