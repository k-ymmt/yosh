#!/bin/sh
# POSIX_REF: 8 Environment Variables - HOME
# DESCRIPTION: tilde expands to $HOME
# EXPECT_OUTPUT: /custom/home
# EXPECT_EXIT: 0
HOME=/custom/home
echo ~
