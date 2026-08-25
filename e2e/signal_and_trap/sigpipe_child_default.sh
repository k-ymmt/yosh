#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: Children inherit SIGPIPE at default disposition (not the Rust runtime's SIG_IGN)
# EXPECT_OUTPUT: 141
# EXPECT_EXIT: 0
sh -c 'kill -PIPE $$; echo survived'
echo $?
