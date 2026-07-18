#!/bin/sh
# POSIX_REF: 2.12 Signals and Error Handling
# DESCRIPTION: EXIT trap set inside a pipeline member fires when that member exits (bash stance)
# EXPECT_OUTPUT: t
{ trap 'echo t' EXIT; :; } | cat
