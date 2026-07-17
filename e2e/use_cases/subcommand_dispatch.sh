#!/bin/sh
# POSIX_REF: 2.9.4.5 case Conditional Construct
# DESCRIPTION: Use case - dispatch on a subcommand argument like a mini CLI tool
# EXPECT_OUTPUT<<END
# starting service
# service is running
# unknown command: bogus
# END
run() {
  case $1 in
    start)  echo "starting service" ;;
    stop)   echo "stopping service" ;;
    status) echo "service is running" ;;
    *)      echo "unknown command: $1" ;;
  esac
}
run start
run status
run bogus
