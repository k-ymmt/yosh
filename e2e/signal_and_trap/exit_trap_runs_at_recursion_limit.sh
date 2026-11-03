#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: An EXIT trap fired by exit at the deepest function level the recursion guard accepted still runs, function calls included, and the exit status is kept
# EXPECT_OUTPUT<<END
# below limit ok
# at limit ok
# END
# EXPECT_EXIT: 0
# The deepest accepted depth depends on the build, so find it by binary
# search: `run N` recurses N levels and exits 7 at the bottom. Past the
# limit the function guard fails (status 1, its diagnostic, then the
# trap); at the limit the trap must still run and keep status 7 — it
# used to be refused with "trap: maximum nesting level exceeded".
run() {
  r=$( (trap 'g' EXIT
        g() { echo trapped; }
        f() { if [ "$1" -gt 0 ]; then f $(($1 - 1)); else exit 7; fi; }
        f "$1") 2>&1 )
  st=$?
}
lo=0 hi=100000
while [ $((hi - lo)) -gt 1 ]; do
  mid=$(((lo + hi) / 2))
  run "$mid"
  case $r in
  *"f: maximum nesting level exceeded"*) hi=$mid ;;
  *) lo=$mid ;;
  esac
done
for n in $((lo - 1)) "$lo"; do
  run "$n"
  if [ "$st" = 7 ] && [ "$r" = trapped ]; then
    [ "$n" = "$lo" ] && echo "at limit ok" || echo "below limit ok"
  else
    echo "depth $n: status $st, output: $r"
  fi
done
