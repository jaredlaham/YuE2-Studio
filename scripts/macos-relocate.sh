#!/bin/bash
# Makes the engine binaries in DIRECTORY load their libraries from beside
# themselves. CMake links them with the build folder as their run path, which
# only exists on the machine that built them; on any other Mac the engine then
# aborts at launch because dyld cannot find libggml. Every absolute run path
# is replaced by @loader_path, and each file is re-signed ad hoc, since editing
# the load commands breaks its signature and Apple Silicon refuses to run an
# unsigned binary.
#
# Usage: scripts/macos-relocate.sh DIRECTORY
set -euo pipefail

directory="${1:?usage: macos-relocate.sh DIRECTORY}"

for file in "$directory"/*; do
    [ -f "$file" ] && [ ! -L "$file" ] || continue
    file "$file" | grep -q 'Mach-O' || continue
    changed=""
    while read -r rpath; do
        case "$rpath" in
            @*) ;;
            *) install_name_tool -delete_rpath "$rpath" "$file"; changed=1 ;;
        esac
    done < <(otool -l "$file" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }')
    if ! otool -l "$file" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }' | grep -qx '@loader_path'; then
        install_name_tool -add_rpath @loader_path "$file"
        changed=1
    fi
    if [ -n "$changed" ]; then
        codesign --force --sign - "$file"
    fi
done
