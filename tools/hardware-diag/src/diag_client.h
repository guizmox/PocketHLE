/* Optional live reporting for the unified launcher. Does not alter assertions. */
#include "imports.h"
static void hardware_diag_progress(const char *name, unsigned int state) {
    unsigned int error = GetLastError(), h, count, n = 0, i = 0;
    char text[240];
    const char *prefix = state == 2 ? "STEP " : state ? "PASS " : "FAIL ";
    while (*prefix) text[n++] = *prefix++;
    while (name[i] && n < sizeof(text)-3) text[n++] = name[i++];
    text[n++] = '\r'; text[n++] = '\n';
    h = CreateFileW(L"\\Flash Disk\\HARDWARELIVE.TXT", 0x40000000, 3, 0, 4, 0, 0);
    if (h != 0xffffffff) {
        SetFilePointer(h, 0, 0, 2);
        WriteFile(h, text, n, &count, 0);
        CloseHandle(h);
    }
    SetLastError(error);
}
/* The launcher supplies the visible console and final result screen. */
static unsigned int hardware_diag_message(unsigned int window, const unsigned short *text,
        const unsigned short *title, unsigned int flags) {
    (void)window; (void)text; (void)title; (void)flags;
    return 1;
}
#undef MessageBoxW
#define MessageBoxW hardware_diag_message
