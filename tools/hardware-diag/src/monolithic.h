/* Adapters used only by the integrated diagnostic. Fixtures keep their ABI. */
#include "imports.h"
unsigned int hardware_diag_spawn(const unsigned short*,const unsigned short*,void*,void*,unsigned int,unsigned int,void*,void*,void*,void*);
const unsigned short *hardware_diag_command(void);
unsigned int hardware_diag_load_fixture(const unsigned short*);
#ifdef HARDWARE_DIAG_MONOLITHIC
#undef CreateProcessW
#define CreateProcessW hardware_diag_spawn
#undef GetCommandLineW
#define GetCommandLineW hardware_diag_command
#ifdef HARDWARE_DIAG_RAM
#undef LoadLibraryW
#define LoadLibraryW hardware_diag_load_fixture
#endif
#endif
