/* Code-hook cache semantics and throughput, using the bundled native engine.
 * cc -O2 -Ivendor/unicorn-engine-sys/include tools/tests/unicorn-code-hooks.c \
 *    -L<unicorn-build> -Wl,-rpath,<unicorn-build> -lunicorn -o code-hooks-test
 */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unicorn/unicorn.h>
#include <unicorn/arm.h>
#include <unicorn/arm64.h>
#include <unicorn/mips.h>

static void ok(uc_err e) { assert(e == UC_ERR_OK); }
static void empty(uc_engine *uc, uint64_t address, uint32_t size, void *data) {
    (void)uc; (void)address; (void)size; (void)data;
}
static void fillers(uc_engine *uc, unsigned n) {
    for (unsigned i = 0; i < n; ++i) {
        uc_hook h;
        ok(uc_hook_add(uc, &h, UC_HOOK_CODE, (void *)empty, NULL,
                       0x3000 + i * 4, 0x3000 + i * 4));
    }
}
static uc_engine *arm(void) {
    uc_engine *uc;
    ok(uc_open(UC_ARCH_ARM, UC_MODE_ARM, &uc));
    ok(uc_mem_map(uc, 0x1000, 0x4000, UC_PROT_ALL));
    uint32_t code[] = {0xe3a00007, 0xe12fff1e};
    ok(uc_mem_write(uc, 0x1000, code, sizeof(code)));
    fillers(uc, 32);
    return uc;
}
struct state;
struct callback { struct state *s; unsigned id; };
struct state {
    unsigned order[32], count, action;
    uc_hook second, added;
    struct callback a, b, c;
};
static void record(uc_engine *uc, uint64_t address, uint32_t size, void *data) {
    struct callback *cb = data;
    struct state *s = cb->s;
    assert(address == 0x1000 && size == 4 && s->count < 32);
    s->order[s->count++] = cb->id;
    if (cb->id == 1) {
        unsigned action = s->action; s->action = 0;
        if (action == 1) {
            ok(uc_hook_add(uc, &s->added, UC_HOOK_CODE, (void *)record,
                           &s->c, 0x1000, 0x1000));
        } else if (action == 2) {
            ok(uc_hook_del(uc, s->second));
        } else if (action == 3) {
            ok(uc_emu_stop(uc));
        }
    }
}
static void run(uc_engine *uc, struct state *s, const unsigned *order, unsigned n) {
    uint32_t lr = 0x1008;
    s->count = 0;
    ok(uc_reg_write(uc, UC_ARM_REG_LR, &lr));
    ok(uc_emu_start(uc, 0x1000, 0x1008, 0, 0));
    assert(s->count == n);
    if (n != 0) { assert(memcmp(s->order, order, n * sizeof(*order)) == 0); }
}
static void semantics(void) {
    uc_engine *uc = arm();
    struct state s = {0};
    s.a = (struct callback){&s, 1};
    s.b = (struct callback){&s, 2};
    s.c = (struct callback){&s, 3};
    uc_hook first;
    ok(uc_hook_add(uc, &first, UC_HOOK_CODE, (void *)record, &s.a, 0x1000, 0x1004));
    /* Only execute the overlapping first word, then return before word two. */
    ok(uc_hook_del(uc, first));
    ok(uc_hook_add(uc, &first, UC_HOOK_CODE, (void *)record, &s.a, 0x1000, 0x1000));
    ok(uc_hook_add(uc, &s.second, UC_HOOK_CODE, (void *)record, &s.b, 0x1000, 0x1000));
    const unsigned ab[] = {1, 2}, abc[] = {1, 2, 3}, ac[] = {1, 3}, a[] = {1};
    run(uc, &s, ab, 2); run(uc, &s, ab, 2);
    s.action = 1; run(uc, &s, abc, 3); run(uc, &s, abc, 3);
    s.action = 2; run(uc, &s, ac, 2); run(uc, &s, ac, 2);
    s.action = 3; run(uc, &s, a, 1); run(uc, &s, ac, 2);
    ok(uc_hook_del(uc, s.added)); run(uc, &s, a, 1);
    ok(uc_hook_del(uc, first));
    for (unsigned i = 0; i < 100; ++i) {
        ok(uc_hook_add(uc, &first, UC_HOOK_CODE, (void *)record, &s.a, 0x1000, 0x1000));
        run(uc, &s, a, 1); run(uc, &s, a, 1);
        ok(uc_hook_del(uc, first));
        run(uc, &s, NULL, 0);
    }
    /* Count-mode insertion/removal cannot reuse a cached pre-counter list. */
    uint32_t lr = 0x1008; ok(uc_reg_write(uc, UC_ARM_REG_LR, &lr));
    ok(uc_emu_start(uc, 0x1000, 0x1008, 0, 1));
    uint32_t pc; ok(uc_reg_read(uc, UC_ARM_REG_PC, &pc)); assert(pc == 0x1004);
    run(uc, &s, NULL, 0);
    ok(uc_close(uc));
}
static void count(uc_engine *uc, uint64_t address, uint32_t size, void *data) {
    (void)uc; (void)address; (void)size; ++*(unsigned *)data;
}
static void other_arch(uc_arch arch, uc_mode mode, const uint32_t *code,
                       unsigned bytes, int lr_id, uint64_t end) {
    uc_engine *uc; ok(uc_open(arch, mode, &uc));
    ok(uc_mem_map(uc, 0x1000, 0x4000, UC_PROT_ALL));
    ok(uc_mem_write(uc, 0x1000, code, bytes)); fillers(uc, 32);
    unsigned calls = 0; uc_hook hook;
    ok(uc_hook_add(uc, &hook, UC_HOOK_CODE, (void *)count, &calls, 0x1000, 0x1000));
    for (unsigned i = 0; i < 20; ++i) {
        ok(uc_reg_write(uc, lr_id, &end));
        ok(uc_emu_start(uc, 0x1000, end, 0, 0));
    }
    assert(calls == 20); ok(uc_close(uc));
}
static void wildcard_and_independent_engines(void) {
    uc_engine *left = arm(), *right = arm();
    unsigned l = 0, r = 0, global = 0;
    uc_hook a, b, all;
    ok(uc_hook_add(left, &a, UC_HOOK_CODE, (void *)count, &l, 0x1000, 0x1000));
    ok(uc_hook_add(right, &b, UC_HOOK_CODE, (void *)count, &r, 0x1000, 0x1000));
    ok(uc_hook_add(left, &all, UC_HOOK_CODE, (void *)count, &global, 1, 0));
    uint32_t lr = 0x1008;
    for (unsigned i = 0; i < 10; ++i) {
        ok(uc_reg_write(left, UC_ARM_REG_LR, &lr));
        ok(uc_reg_write(right, UC_ARM_REG_LR, &lr));
        ok(uc_emu_start(left, 0x1000, 0x1008, 0, 0));
        ok(uc_emu_start(right, 0x1000, 0x1008, 0, 0));
    }
    assert(l == 10 && r == 10 && global == 20);
    ok(uc_hook_del(left, all));
    ok(uc_emu_start(left, 0x1000, 0x1008, 0, 0));
    assert(l == 11 && r == 10 && global == 20);
    ok(uc_close(left)); ok(uc_close(right));
}
static void stop_in_thumb_it(uc_engine *uc, uint64_t address, uint32_t size, void *data) {
    (void)address; (void)size; ++*(unsigned *)data; ok(uc_emu_stop(uc));
}
static void thumb_it(void) {
    uc_engine *uc; ok(uc_open(UC_ARCH_ARM, UC_MODE_THUMB, &uc));
    ok(uc_mem_map(uc, 0x1000, 0x4000, UC_PROT_ALL)); fillers(uc, 32);
    const uint16_t code[] = {0x2000, 0xbf04, 0x2101, 0x2202, 0x2303, 0x4770};
    ok(uc_mem_write(uc, 0x1000, code, sizeof(code)));
    unsigned stops = 0, followers = 0; uc_hook a, b;
    ok(uc_hook_add(uc, &a, UC_HOOK_CODE, (void *)stop_in_thumb_it, &stops, 0x1004, 0x1004));
    ok(uc_hook_add(uc, &b, UC_HOOK_CODE, (void *)count, &followers, 0x1004, 0x1004));
    for (unsigned i = 0; i < 10; ++i) {
        uint32_t lr = 0x100d, value;
        ok(uc_reg_write(uc, UC_ARM_REG_LR, &lr));
        ok(uc_emu_start(uc, 0x1001, 0x100c, 0, 0));
        ok(uc_reg_read(uc, UC_ARM_REG_R1, &value)); assert(value == 1);
        ok(uc_reg_read(uc, UC_ARM_REG_R2, &value)); assert(value == 2);
    }
    assert(stops == 10 && followers == 10);
    ok(uc_close(uc));
}
static void full_width_addresses(void) {
    uc_engine *uc; ok(uc_open(UC_ARCH_ARM64, UC_MODE_ARM, &uc));
    const uint64_t low = 0x1000, high = UINT64_C(0x100001000);
    const uint32_t code[] = {0x528000e0, 0xd65f03c0};
    ok(uc_mem_map(uc, low, 0x1000, UC_PROT_ALL));
    ok(uc_mem_map(uc, high, 0x1000, UC_PROT_ALL));
    ok(uc_mem_write(uc, low, code, sizeof(code)));
    ok(uc_mem_write(uc, high, code, sizeof(code)));
    fillers(uc, 32);
    unsigned a = 0, b = 0; uc_hook ha, hb;
    ok(uc_hook_add(uc, &ha, UC_HOOK_CODE, (void *)count, &a, low, low));
    ok(uc_hook_add(uc, &hb, UC_HOOK_CODE, (void *)count, &b, high, high));
    for (unsigned i = 0; i < 20; ++i) {
        uint64_t base = i & 1 ? low : high, end = base + 8;
        ok(uc_reg_write(uc, UC_ARM64_REG_X30, &end));
        ok(uc_emu_start(uc, base, end, 0, 0));
    }
    assert(a == 10 && b == 10); ok(uc_close(uc));
}
struct nested_state { unsigned order[4], used, action; uc_hook victim; };
static void inner(uc_engine *uc, uint64_t address, uint32_t size, void *data) {
    (void)uc; (void)size; struct nested_state *s = data;
    assert(address == 0x1400 && s->used < 4); s->order[s->used++] = 9;
    if (s->action == 1) { ok(uc_hook_del(uc, s->victim)); s->action = 2; }
}
static void outer(uc_engine *uc, uint64_t address, uint32_t size, void *data) {
    (void)size; struct nested_state *s = data;
    assert(address == 0x1000 && s->used < 3); s->order[s->used++] = 1;
    uc_context *context; ok(uc_context_alloc(uc, &context));
    ok(uc_context_save(uc, context));
    uint32_t lr = 0x1408; ok(uc_reg_write(uc, UC_ARM_REG_LR, &lr));
    ok(uc_emu_start(uc, 0x1400, 0x1408, 0, 0));
    if (s->action == 2) {
        s->action = 0;
        /* A second nested start frees the removed outer-list node. */
        ok(uc_emu_start(uc, 0x1400, 0x1408, 0, 0));
    }
    ok(uc_context_restore(uc, context)); ok(uc_context_free(context));
}
static void after_outer(uc_engine *uc, uint64_t address, uint32_t size, void *data) {
    (void)uc; (void)size; struct nested_state *s = data;
    assert(address == 0x1000 && s->used < 3); s->order[s->used++] = 2;
}
static void nested_callbacks(void) {
    uc_engine *uc = arm();
    uint32_t code[] = {0xe3a00009, 0xe12fff1e};
    ok(uc_mem_write(uc, 0x1400, code, sizeof(code)));
    struct nested_state s = {0}; uc_hook a, b, c;
    ok(uc_hook_add(uc, &a, UC_HOOK_CODE, (void *)outer, &s, 0x1000, 0x1000));
    ok(uc_hook_add(uc, &b, UC_HOOK_CODE, (void *)inner, &s, 0x1400, 0x1400));
    ok(uc_hook_add(uc, &c, UC_HOOK_CODE, (void *)after_outer, &s, 0x1000, 0x1000));
    for (unsigned i = 0; i < 10; ++i) {
        uint32_t lr = 0x1008; s.used = 0;
        ok(uc_reg_write(uc, UC_ARM_REG_LR, &lr));
        ok(uc_emu_start(uc, 0x1000, 0x1008, 0, 0));
        assert(s.used == 3 && s.order[0] == 1 && s.order[1] == 9 && s.order[2] == 2);
    }
    s.used = 0; s.action = 1; s.victim = c;
    uint32_t lr = 0x1008; ok(uc_reg_write(uc, UC_ARM_REG_LR, &lr));
    ok(uc_emu_start(uc, 0x1000, 0x1008, 0, 0));
    assert(s.used == 3 && s.order[0] == 1 && s.order[1] == 9 && s.order[2] == 9);
    s.used = 0;
    ok(uc_emu_start(uc, 0x1000, 0x1008, 0, 0));
    assert(s.used == 2 && s.order[0] == 1 && s.order[1] == 9);
    ok(uc_close(uc));
}
static void benchmark(void) {
    uc_engine *uc = arm();
    uint32_t loop[] = {0xeb00003e, 0xe2544001, 0x1afffffc};
    uint32_t ret = 0xe12fff1e;
    ok(uc_mem_write(uc, 0x1000, loop, sizeof(loop)));
    ok(uc_mem_write(uc, 0x1100, &ret, sizeof(ret)));
    fillers(uc, 512);
    unsigned calls = 0; uc_hook hook;
    ok(uc_hook_add(uc, &hook, UC_HOOK_CODE, (void *)count, &calls, 0x1100, 0x1100));
    uint32_t iterations = 200000;
    ok(uc_reg_write(uc, UC_ARM_REG_R4, &iterations));
    clock_t start = clock();
    ok(uc_emu_start(uc, 0x1000, 0x100c, 0, 0));
    double seconds = (double)(clock() - start) / CLOCKS_PER_SEC;
    assert(calls == iterations);
    printf("%u code-hook calls: %.6f CPU seconds\n", calls, seconds);
    ok(uc_close(uc));
}
int main(void) {
    semantics(); wildcard_and_independent_engines(); thumb_it(); full_width_addresses(); nested_callbacks();
    const uint32_t a64[] = {0x528000e0, 0xd65f03c0};
    other_arch(UC_ARCH_ARM64, UC_MODE_ARM, a64, sizeof(a64), UC_ARM64_REG_X30, 0x1008);
    const uint32_t mips[] = {0x24020007, 0x03e00008, 0};
    other_arch(UC_ARCH_MIPS, UC_MODE_MIPS32 | UC_MODE_LITTLE_ENDIAN, mips,
               sizeof(mips), UC_MIPS_REG_RA, 0x100c);
    benchmark(); puts("Code-hook semantics passed");
    return 0;
}
