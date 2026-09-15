# large exported environment + external spawns
i=0
while [ "$i" -lt 300 ]; do export "EV_$i=value_number_$i"; i=$((i + 1)); done
i=0
while [ "$i" -lt 300 ]; do /usr/bin/true; i=$((i + 1)); done
echo "$i"
