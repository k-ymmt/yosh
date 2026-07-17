#!/bin/sh
# POSIX_REF: 2.9.4 Compound Commands - for
# DESCRIPTION: for word list must be terminated by ; or newline before do
# EXPECT_STDERR: expected 'do'
# EXPECT_EXIT: 2
for i in a b do echo x; done
