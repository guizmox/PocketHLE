/* Standalone regression test for the bundled Unicorn engine (ARM only).
 * cc -Ivendor/unicorn-engine-sys/include tools/tests/unicorn-optimizations.c \
 *    -L<unicorn-build> -Wl,-rpath,<unicorn-build> -lunicorn -o optimizations-test
 */
#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <unicorn/unicorn.h>
#include <unicorn/arm.h>

#define OPTIONS UC_CTL_POCKETHLE_OPTIMIZATIONS
static void ok(uc_err error) { assert(error == UC_ERR_OK); }
static void reg(uc_engine *uc, int id, uint32_t value) {
    ok(uc_reg_write(uc, id, &value));
}
static uint32_t r0(uc_engine *uc) {
    uint32_t value; ok(uc_reg_read(uc, UC_ARM_REG_R0, &value)); return value;
}
static void writes(uc_engine *uc, uc_mem_type type, uint64_t address,
                   int size, int64_t value, void *opaque) {
    (void)uc; (void)type; (void)address; (void)size; (void)value;
    ++*(unsigned *)opaque;
}
int main(void) {
    for (unsigned flags = 0; flags < 8; ++flags) {
        uc_engine *uc; uint32_t reported = 0;
        ok(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
        ok(uc_ctl(uc, UC_CTL_READ(OPTIONS, 1), &reported));
        assert(reported == 7);
        assert(uc_ctl(uc, UC_CTL_WRITE(OPTIONS, 1), 8u) == UC_ERR_ARG);
        ok(uc_ctl(uc, UC_CTL_WRITE(OPTIONS, 1), flags));
        ok(uc_ctl(uc, UC_CTL_READ(OPTIONS, 1), &reported));
        assert(reported == flags);
        ok(uc_mem_map(uc, 0x1000, 0x4000, UC_PROT_ALL));
        assert(uc_ctl(uc, UC_CTL_WRITE(OPTIONS, 1), flags) == UC_ERR_ARG);
        /* Private executable RAM data writes still invoke the write hook. */
        uint32_t store[] = {0xe5810000, 0xe12fff1e}; /* str r0,[r1]; bx lr */
        ok(uc_mem_write(uc, 0x1000, store, sizeof(store)));
        reg(uc, UC_ARM_REG_R1, 0x2000); reg(uc, UC_ARM_REG_LR, 0x1008);
        unsigned count = 0; uc_hook hook;
        ok(uc_hook_add(uc, &hook, UC_HOOK_MEM_WRITE, (void *)writes, &count, 1, 0));
        for (unsigned i = 1; i <= 4; ++i) {
            reg(uc, UC_ARM_REG_R0, i);
            ok(uc_emu_start(uc, 0x1000, 0x1008, 0, 0));
        }
        uint32_t data; ok(uc_mem_read(uc, 0x2000, &data, sizeof(data)));
        assert(data == 4 && count == 4);
        ok(uc_hook_del(uc, hook));
        ok(uc_mem_protect(uc, 0x2000, 0x1000, UC_PROT_READ));
        assert(uc_emu_start(uc, 0x1000, 0x1008, 0, 0) == UC_ERR_WRITE_PROT);
        ok(uc_mem_protect(uc, 0x2000, 0x1000, UC_PROT_ALL));
        /* A guest store must invalidate code already translated on that page. */
        uint32_t code[] = {0xe3a00001, 0xe12fff1e};
        ok(uc_mem_write(uc, 0x4000, code, sizeof(code)));
        reg(uc, UC_ARM_REG_LR, 0x4008);
        ok(uc_emu_start(uc, 0x4000, 0x4008, 0, 0)); assert(r0(uc) == 1);
        reg(uc, UC_ARM_REG_R0, 0xe3a00002); reg(uc, UC_ARM_REG_R1, 0x4000);
        reg(uc, UC_ARM_REG_LR, 0x1008);
        ok(uc_emu_start(uc, 0x1000, 0x1008, 0, 0));
        reg(uc, UC_ARM_REG_LR, 0x4008);
        ok(uc_emu_start(uc, 0x4000, 0x4008, 0, 0)); assert(r0(uc) == 2);
        /* Unmapping/remapping must discard cached destinations and translations. */
        ok(uc_mem_unmap(uc, 0x4000, 0x1000));
        ok(uc_mem_map(uc, 0x4000, 0x1000, UC_PROT_ALL));
        code[0] = 0xe3a00003;
        ok(uc_mem_write(uc, 0x4000, code, sizeof(code)));
        ok(uc_emu_start(uc, 0x4000, 0x4008, 0, 0)); assert(r0(uc) == 3);
        /* Shared host RAM aliases retain conservative invalidation behavior. */
        void *shared = calloc(1, 0x1000); assert(shared);
        ok(uc_mem_map_ptr(uc, 0x6000, 0x1000, UC_PROT_ALL, shared));
        ok(uc_mem_map_ptr(uc, 0x7000, 0x1000, UC_PROT_ALL, shared));
        ok(uc_mem_write(uc, 0x6000, code, sizeof(code)));
        reg(uc, UC_ARM_REG_LR, 0x6008);
        ok(uc_emu_start(uc, 0x6000, 0x6008, 0, 0)); assert(r0(uc) == 3);
        reg(uc, UC_ARM_REG_R0, 0xe3a00004); reg(uc, UC_ARM_REG_R1, 0x7000);
        reg(uc, UC_ARM_REG_LR, 0x1008);
        ok(uc_emu_start(uc, 0x1000, 0x1008, 0, 0));
        reg(uc, UC_ARM_REG_LR, 0x6008);
        ok(uc_emu_start(uc, 0x6000, 0x6008, 0, 0)); assert(r0(uc) == 4);
        ok(uc_close(uc)); free(shared);
    }
    puts("All 8 native optimization combinations passed.");
    return 0;
}
