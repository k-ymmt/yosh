#!/bin/sh
# POSIX_REF: 4 Utilities - printf
# DESCRIPTION: printf %b interprets escapes and \c stops output; * width and precision
# EXPECT_OUTPUT<<END
# aA	b
#     3|3    |3.14
# stopEND
# END
printf '%b\n' 'a\0101\tb'
printf '%*d|%-*d|%.*f\n' 5 3 5 3 2 3.14159
printf '%b rest\n' 'stop\c'; echo END
