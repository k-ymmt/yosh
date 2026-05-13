#!/bin/sh
# POSIX_REF: 2.5.2 Special Parameters
# DESCRIPTION: $$ is set to a non-empty integer (shell process ID)
# EXPECT_OUTPUT: ok
# EXPECT_EXIT: 0
case "$$" in
    ''|*[!0-9]*) echo "bad pid: $$" ;;
    *) echo ok ;;
esac
