# Localized MSVC banners may put "Microsoft" after translated words.
# The upstream anchored check misses French cl.exe even with VSLANG=1033
# when the English language resources are not installed.
s/grep -q \^Microsoft/grep -q Microsoft/g
s/grep \^Microsoft/grep Microsoft/g
# The selected MSVC linker is FFmpeg's mslink wrapper. Identify that wrapper
# by its basename, rather than requiring the English word "Linker".
/if \$_cc -nologo- 2>\&1 | grep -q Linker; then/c\
        if [ "${_cc##*/}" = mslink ] || [ "${_cc##*/}" = link.exe ] || $_cc -nologo- 2>\&1 | grep -q Linker; then
