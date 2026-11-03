#!/bin/sh
# POSIX_REF: 2.9.5 Function Definition Command
# DESCRIPTION: A deeply nested eval parse, expansion or arithmetic deep in bounded recursion fails with a shell error, never a native stack overflow
# EXPECT_OUTPUT<<END
# parse 30
# arith 30
# END
# EXPECT_EXIT: 0
# The recursion depths sweep across the point where the remaining stack
# runs out; every run must end in its EXIT trap (output or a shell error),
# never SIGABRT. Which depths succeed depends on the build.
unset x
word=ok
i=0
while [ "$i" -lt 350 ]; do word='${x:-'"$word"'}'; i=$((i+1)); done
code="echo $word;"
i=0
while [ "$i" -lt 100 ]; do code="{ $code };"; i=$((i+1)); done
e=1
i=0
while [ "$i" -lt 250 ]; do e="($e)"; i=$((i+1)); done
acode="echo \$(($e));"
i=0
while [ "$i" -lt 150 ]; do acode="{ $acode };"; i=$((i+1)); done
f() { if [ "$1" -gt 0 ]; then f $(($1 - 1)) "$2"; else eval "$2"; fi; }
for kind in parse arith; do
  if [ "$kind" = parse ]; then c=$code; else c=$acode; fi
  n=0 trapped=0
  while [ "$n" -lt 450 ]; do
    r=$( (trap 'echo trapped' EXIT; f "$n" "$c" >/dev/null) 2>/dev/null )
    [ "$r" = trapped ] && trapped=$((trapped + 1))
    n=$((n + 15))
  done
  echo "$kind $trapped"
done
