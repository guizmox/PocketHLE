# MSVC may emit a localized compiler banner in a legacy code page even with
# VSLANG=1033. Preserve each byte of generated C string macros using standard
# three-digit octal escapes, so config.h does not depend on the source charset.
# Run with LC_ALL=C: awk must iterate over bytes rather than Unicode characters.
BEGIN {
    for (i = 128; i < 256; i++)
        escaped[sprintf("%c", i)] = sprintf("\\%03o", i)
}
/^#define [A-Za-z0-9_]+[ \t]+"/ {
    output = ""
    for (i = 1; i <= length($0); i++) {
        ch = substr($0, i, 1)
        output = output ((ch in escaped) ? escaped[ch] : ch)
    }
    print output
    next
}
{ print }
