#include <stdint.h>
#include "qemu/osdep.h"
#include "qemu-common.h"
#include "exec/exec-all.h"
#include "uc_priv.h"

#include <stdio.h>

static void raise_mmu_exception(CPUState *cs, target_ulong address,
                                int rw, uintptr_t retaddr)
{
    cs->uc->invalid_error = UC_ERR_EXCEPTION;
    cs->uc->invalid_addr = address;
    cpu_exit(cs->uc->cpu);
    cpu_loop_exit_restore(cs, retaddr);
}

static uc_mem_type rw_to_mem_type(int rw)
{
    switch (rw) {
    case MMU_DATA_LOAD:
        return UC_MEM_READ;
    case MMU_DATA_STORE:
        return UC_MEM_WRITE;
    case MMU_INST_FETCH:
        return UC_MEM_FETCH;
    default:
        return UC_MEM_READ;
    }
}

static int perms_to_prot(int perms)
{
    int ret = 0;
    if (perms & UC_PROT_READ) {
        ret |= PAGE_READ;
    }
    if (perms & UC_PROT_WRITE) {
        ret |= PAGE_WRITE;
    }
    if (perms & UC_PROT_EXEC) {
        ret |= PAGE_EXEC;
    }
    return ret;
}

bool unicorn_fill_tlb(CPUState *cs, vaddr address, int size,
                      MMUAccessType rw, int mmu_idx,
                      bool probe, uintptr_t retaddr)
{
    bool handled = false;
    bool ret = false;
    struct uc_struct *uc = cs->uc;
    uc_tlb_entry e;
    struct hook *hook;
    HOOK_FOREACH_VAR_DECLARE;

    /* Preserve MIPS32's architectural kernel-segment translation, including
       the cached and uncached aliases of low physical memory. */
    if (uc->ph_mips_mapping_tlb && address >= UINT64_C(0x80000000)) {
        CPUClass *cc = CPU_GET_CLASS(cs);
        return cc->tlb_fill_cpu(cs, address, size, rw, mmu_idx, probe, retaddr);
    }

    /* PocketHLE's MIPS refill callback only resolves flat mappings.
       Restoring the faulting instruction here resurrects a pending
       branch after its delay-slot store succeeds. Faults still restore
       state in raise_mmu_exception. */
    if (!uc->ph_mips_mapping_tlb) {
        cpu_restore_state(cs, retaddr, false);
    }

    HOOK_FOREACH(uc, hook, UC_HOOK_TLB_FILL) {
        if (hook->to_delete) {
            continue;
        }
        if (!HOOK_BOUND_CHECK(hook, address)) {
            continue;
        }
        handled = true;
        JIT_CALLBACK_GUARD_VAR(ret, ((uc_cb_tlbevent_t)hook->callback)(uc, address & TARGET_PAGE_MASK, rw_to_mem_type(rw), &e, hook->user_data));
        if (ret) {
            break;
        }
    }

    if (handled && !ret) {
        goto tlb_miss;
    }

    if (!handled) {
        e.paddr = address & TARGET_PAGE_MASK;
        switch (rw) {
        case MMU_DATA_LOAD:
            e.perms = UC_PROT_READ;
            break;
        case MMU_DATA_STORE:
            e.perms = UC_PROT_WRITE;
            break;
        case MMU_INST_FETCH:
            e.perms = UC_PROT_EXEC;
            break;
        default:
            e.perms = 0;
            break;
        }
    }

    switch (rw) {
    case MMU_DATA_LOAD:
        ret = e.perms & UC_PROT_READ;
        break;
    case MMU_DATA_STORE:
        ret = e.perms & UC_PROT_WRITE;
        break;
    case MMU_INST_FETCH:
        ret = e.perms & UC_PROT_EXEC;
        break;
    default:
        ret = false;
        break;
    }

    if (ret) {
        tlb_set_page(cs, address & TARGET_PAGE_MASK, e.paddr & TARGET_PAGE_MASK, perms_to_prot(e.perms), mmu_idx, TARGET_PAGE_SIZE);
        return true;
    }

tlb_miss:
    if (probe) {
        return false;
    }
    raise_mmu_exception(cs, address, rw, retaddr);
    return false;
}
