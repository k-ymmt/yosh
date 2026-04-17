#!/bin/sh
# POSIX_REF: 2.9.1 Simple Commands
# DESCRIPTION: command -v reports alias in POSIX form
# EXPECT_OUTPUT: alias ll='ls -l'
# EXPECT_EXIT: 0
alias ll='ls -l'
command -v ll
