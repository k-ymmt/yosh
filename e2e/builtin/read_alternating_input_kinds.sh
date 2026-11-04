#!/bin/sh
# POSIX_REF: 4 Utilities - read
# DESCRIPTION: read alternating between a regular file, a pipe, a here-document and /dev/null in one shell consumes exactly one line of each
# EXPECT_OUTPUT<<END
# f1 p1 h1 n=
# f2 p1 h1 n=
# rest=f3
# END
# EXPECT_EXIT: 0
cd "$TEST_TMPDIR" || exit 1
printf 'f1\nf2\nf3\n' > f
mkfifo p
exec 3<f
for i in 1 2; do
    read -r a <&3
    printf 'p1\np2\n' > p &
    read -r b < p
    wait
    read -r c <<EOF
h1
h2
EOF
    n=x
    read -r n </dev/null
    echo "$a $b $c n=$n"
done
read -r rest <&3
echo "rest=$rest"
