#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: An EXIT trap fired by exit at the deepest function level the recursion guard accepted still runs, function calls and command substitutions included, and the exit status is kept
# EXPECT_OUTPUT<<END
# g below limit ok
# g at limit ok
# echo "$(g)" below limit ok
# echo "$(g)" at limit ok
# END
# EXPECT_EXIT: 0
# The deepest accepted depth depends on the build, so find it by binary
# search: `run ACTION N` recurses N levels and exits 7 at the bottom. Past
# the limit the function guard fails (status 1, its diagnostic, then the
# trap); at the limit the trap must still run and keep status 7 — it
# used to be refused with "trap: maximum nesting level exceeded", and a
# command substitution in the action lost the trap's stack allowance
# ("g: maximum nesting level exceeded").
run() {
  r=$( (trap "$1" EXIT
        g() { echo trapped; }
        f() { if [ "$1" -gt 0 ]; then f $(($1 - 1)); else exit 7; fi; }
        f "$2") 2>&1 )
  st=$?
}
lo=0 hi=100000
while [ $((hi - lo)) -gt 1 ]; do
  mid=$(((lo + hi) / 2))
  run g "$mid"
  case $r in
  *"f: maximum nesting level exceeded"*) hi=$mid ;;
  *) lo=$mid ;;
  esac
done
# One flat loop: `run` must be called at the same nesting depth as in
# the search above, since the forked subshell starts where its caller
# stood on the stack.
i=0
for n in $((lo - 1)) "$lo" $((lo - 1)) "$lo"; do
  i=$((i + 1))
  if [ "$i" -le 2 ]; then action=g; else action='echo "$(g)"'; fi
  run "$action" "$n"
  if [ "$st" = 7 ] && [ "$r" = trapped ]; then
    [ "$n" = "$lo" ] && echo "$action at limit ok" || echo "$action below limit ok"
  else
    echo "$action depth $n: status $st, output: $r"
  fi
done
