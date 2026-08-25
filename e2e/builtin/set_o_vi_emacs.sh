#!/bin/sh
# POSIX_REF: 2.15 set -o vi
# DESCRIPTION: set -o vi / set -o emacs toggle and mutual exclusion
# EXPECT_OUTPUT<<END
# vi=on
# emacs=off
# vi=off
# emacs=on
# vi=off
# emacs=off
# END
report() {
  set -o | while read -r name state; do
    case "$name" in
    vi | emacs) echo "$name=$state" ;;
    esac
  done
}
set -o vi
report | sort -r
set -o emacs
report | sort -r
set +o emacs
report | sort -r
