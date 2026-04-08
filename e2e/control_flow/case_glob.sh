#!/bin/sh
# POSIX_REF: 2.9.4.5 case Conditional Construct
# DESCRIPTION: case supports glob patterns and multiple patterns with |
# EXPECT_OUTPUT<<END
# glob
# multi
# default
# END
case hello in h*) echo glob ;; esac
case bar in foo|bar|baz) echo multi ;; esac
case xyz in foo) echo wrong ;; *) echo default ;; esac
