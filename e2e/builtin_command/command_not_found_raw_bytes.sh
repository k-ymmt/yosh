#!/bin/sh
# POSIX_REF: 2.8.1 Consequences of Shell Errors
# DESCRIPTION: command, exit and return diagnostics print a name or operand holding a non-UTF-8 byte with its original bytes
# EXPECT_OUTPUT<<END
# command ok
# exit ok
# return ok
# END
# EXPECT_EXIT: 0
d=$(printf 'x\377')
check() {
  case $2 in
  "$3") echo "$1 ok" ;;
  *) printf '%s bad: %s\n' "$1" "$2" | od -c ;;
  esac
}
check command "$(command "$d" 2>&1)" "yosh: command: $d: not found"
check exit "$( (exit "$d") 2>&1)" "yosh: exit: $d: numeric argument required"
check return "$( (f() { return "$d"; }; f) 2>&1)" "yosh: return: $d: numeric argument required"
