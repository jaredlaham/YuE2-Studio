#!/bin/bash
# Fails if any Mach-O file in DIRECTORY names a library or run path outside the
# system and its own folder, so a bundle that would only start on the machine
# that built it is caught before it ships.
#
# Usage: scripts/macos-check-portable.sh DIRECTORY
set -euo pipefail

directory="${1:?usage: macos-check-portable.sh DIRECTORY}"
bad=0
checked=0
for file in "$directory"/*; do
    [ -f "$file" ] && [ ! -L "$file" ] || continue
    file "$file" | grep -q 'Mach-O' || continue
    checked=$((checked + 1))
    while read -r path; do
        case "$path" in
            @rpath/*|@loader_path*|@executable_path*|/usr/lib/*|/System/*) ;;
            *) echo "$(basename "$file") refers to $path"; bad=1 ;;
        esac
    done < <(
        otool -l "$file" | awk '
            /cmd LC_RPATH/ { getline; getline; print $2 }
            /cmd LC_LOAD_DYLIB|cmd LC_LOAD_WEAK_DYLIB|cmd LC_REEXPORT_DYLIB/ { getline; getline; print $2 }'
    )
done
if [ "$checked" -eq 0 ]; then
    echo "no Mach-O files in $directory"
    exit 1
fi
if [ "$bad" -ne 0 ]; then
    exit 1
fi
echo "$checked files in $directory load only system libraries and their neighbours"
