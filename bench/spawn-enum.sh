#!/bin/sh
# The fixed workload behind docs/internals/performance.md — identical in every
# condition, so the only variable is what C: is.
#
#   plain Wine :  WINEPREFIX=/some/prefix sh bench/spawn-enum.sh
#   Raven      :  raven run <env> -- /bin/sh /full/path/to/bench/spawn-enum.sh
#
# Quiet the machine first, and check nothing survived a previous run:
# `pgrep -a wine` must be empty. Prints one parseable line per sample,
# wall-clock milliseconds.
export WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml="
wine cmd /c exit >/dev/null 2>&1   # warm-up: wineserver boots outside the measurement

# Process spawn, 8 samples.
for i in 1 2 3 4 5 6 7 8; do
  s=$(date +%s%N); wine cmd /c exit >/dev/null 2>&1; e=$(date +%s%N)
  echo "SPAWN $(( (e-s)/1000000 ))"
done

# Directory enumeration, 30 dirs of System32 per run, 3 runs. The two
# conditions enumerate directories of different sizes — 817 entries against
# 4877 — so divide by the entry count before comparing (see performance.md).
for r in 1 2 3; do
  s=$(date +%s%N)
  wine cmd /c "for /L %i in (1,1,30) do @dir C:\\windows\\system32 >nul" >/dev/null 2>&1
  e=$(date +%s%N)
  echo "ENUM $(( (e-s)/1000000 ))"
done
wineserver -k 2>/dev/null
