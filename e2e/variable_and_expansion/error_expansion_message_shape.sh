#!/bin/sh
# POSIX_REF: 2.6.2 Parameter Expansion (${parameter:?word})
# DESCRIPTION: ${var:?word} diagnostic includes the variable name
# EXPECT_EXIT: 1
# EXPECT_STDERR: novar: custom message
echo ${novar:?custom message}
