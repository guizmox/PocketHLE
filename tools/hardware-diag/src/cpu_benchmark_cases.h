/* v2 cost probes. Legacy v1's block, score and timing remain separate. */
typedef unsigned long long U64;
#define CPU_CASE_ITERATIONS 1024u
#define CPU_CASE_MS 500u
#define CPU_CASE_MAX_BLOCKS 65536u
static const U32 cpu_inputs32[8] = {
    0x7fffffff, 0x80000001, 0xffffffff, 0x12345678,
    0x01020304, 0xdeadbeef, 0x00010000, 0xfedcba98
};
static const U32 cpu_divisors32[8] = {3, 7, 31, 65537, 257, 11, 17, 65521};
static const U64 cpu_inputs64[8] = {
    0x123456789abcdef0ull, 0x8000000100000001ull,
    0x7ffffffffffffffeull, 0xffffffffffffff01ull, 0x0000000200000001ull,
    0x0000ffff12340000ull, 0xfedcba9876543210ull, 0x00000001ffffffffull
};
static const U64 cpu_divisors64[8] = {
    3, 0x100000001ull, 65537, 17, 0x100000003ull, 257, 65521, 0x10001
};
#ifdef FLOATING
static const char *const cpu_case_names[] = {
    "f32_add_runtime", "f32_multiply_runtime", "f32_divide_runtime",
    "f32_sqrt_runtime", "i32_f32_roundtrip_runtime", "f32_f64_roundtrip_runtime",
    "f64_add_multiply_runtime", "f64_divide_runtime", "f64_sqrt_runtime", "series_f32_runtime"
};
static const U32 cpu_expected[] = {
    0x21f93dc5u, 0x9af9bdc5u, 0xcc59fdc5u, 0xdd5c21c5u, 0xc38145c5u,
    0x5c643dc5u, 0x39443ec5u, 0x6118e8c5u, 0xfd7c3dc5u, 0x54f58aafu
};
static const U32 cpu_calls_per_iteration[] = {1, 1, 1, 1, 2, 2, 2, 1, 1, 7};
/* Normal signed inputs plus zero; sqrt receives the absolute value. */
static const U32 cpu_single_inputs[8] = {
    0x00000000, 0xbfc00000, 0x40500000, 0xc1880000,
    0x42f70000, 0xbd800000, 0x44800000, 0xc27f0000
};
static const U64 cpu_double_inputs[8] = {
    /* Generated from the fixed binary64 reference vector, not at run time. */
    0x54b249ad2594c37dull, 0xab2bff2ee48e0530ull, 0x0000000000000000ull, 0xc00a000000000000ull, 0x423cbe991a144000ull, 0x358dee7a4ad4b81full, 0xca511b0ec57e649aull, 0x404fe00000000000ull
};
#else
static const char *const cpu_case_names[] = {
    "u32_multiply_add", "u32_divide_remainder", "i32_divide_remainder",
    "u64_add_shift", "u64_multiply_runtime", "u64_divide_runtime", "i64_divide_runtime", "series_i32_signed_divide"
};
static const U32 cpu_expected[] = {
    0x331255c5u, 0x41affac5u, 0x1cd7b4c5u, 0x1a9a95c5u,
    0x0246cac5u, 0xe64e09c5u, 0xa184cfc5u, 0x803048f5u
};
static const U32 cpu_calls_per_iteration[] = {0, 1, 1, 0, 1, 1, 1, 1};
#endif
#define CPU_CASE_COUNT (sizeof(cpu_expected) / sizeof(cpu_expected[0]))

__attribute__((noinline)) static U32 cpu_case_block(U32 mode) {
    U32 i, j, h = 0x811c9dc5u;
    U64 value;
    volatile U32 series_sum = 0;
#ifdef FLOATING
    U32 series_sign = 0x3f800000u, series_denominator = 0x3f800000u;
#endif
    for (i = 0; i < CPU_CASE_ITERATIONS; i++) {
        j = i & 7;
#ifdef FLOATING
        U32 bits = cpu_single_inputs[j];
        U64 wide = cpu_double_inputs[j];
        switch (mode) {
        case 0: value = __adds(bits, 0x3fa00000); break;
        case 1: value = __muls(bits, 0x3f400000); break;
        case 2: value = __divs(bits, 0x40500000); break;
        case 3: value = sqrtf(bits & 0x7fffffff); break;
        case 4: value = __stoi(__itos((int)cpu_inputs32[j] >> 4)); break;
        case 5: value = __dtos(__stod(bits)); break;
        case 6: value = __muld(__addd(wide, 0x3ff4000000000000ull), 0x3fe8000000000000ull); break;
        case 7: value = __divd(wide, 0x400a000000000000ull); break;
        case 8: value = sqrt(wide & 0x7fffffffffffffffull); break;
        default: {
            /* Alternating reciprocal series: same runtime operation families
               as GLBenchmark's CPU loop, with our own fixed bounded workload. */
            U32 term = __divs(__muls(series_sign, 0x40800000u), series_denominator);
            series_sum = __adds(series_sum, term);
            series_sign = __subs(0, series_sign);
            series_denominator = __adds(series_denominator, 0x40000000u);
            value = series_sum | ((U64)(__lts(series_denominator, 0x45003000u)
                    | (__nes(series_sum, 0) << 1)) << 32);
            break;
        }
        }
#else
        U32 a = cpu_inputs32[j], d = cpu_divisors32[j];
        U64 b = cpu_inputs64[j], e = cpu_divisors64[j];
        switch (mode) {
        case 0: value = a * 1664525u + 1013904223u; break;
        /* The ARM __rt_*div ABI returns quotient in r0, remainder in r1.
           A two-word return captures BOTH; divisor is the FIRST argument. */
        case 1: value = ((U64 (*)(U32, U32))__rt_udiv)(d, a); break;
        case 2: value = __rt_sdiv((j & 1) ? 0u - d : d, a); break;
        case 3: value = ((b << 7) ^ (b >> 13)) + e; break;
        case 4: value = __ll_mul(b, e); break;
        /* These explicit 64-by-64 helpers take the DIVIDEND first. */
        case 5: value = __rt_udiv64by64(b, e); break;
        case 6: value = __rt_sdiv64by64(b, (j & 1) ? 0ull - e : e); break;
        default: {
            U32 quotient = (U32)__rt_sdiv(2u * i + 1u, 100000000u);
            series_sum = (i & 1u) ? series_sum - quotient : series_sum + quotient;
            value = series_sum;
            break;
        }
        }
#endif
        /* Every result contributes, including the upper 32 bits/remainder.
           Fixed hashing/loop/load cost is included, never subtracted. */
        h = (h ^ (U32)value) * 16777619u;
        h = (h ^ (U32)(value >> 32)) * 16777619u;
    }
    result_sink = h;
    return h;
}

static U32 cpu_cases(void) {
    U32 c, pass, start, elapsed, blocks, value, rates[3], tmp, failed = 0;
    line("--- Additional CPU cost probes v2 ---");
    line("Legacy final score above is unchanged; these have separate per-case results.");
    line("ARM guest, integer + WinCE soft-float ABI. Not native host CPU/FPU performance.");
    line("Each rate includes loop, input loads and checksum cost. Runtime calls are identified.");
    line("Three samples, target 500ms, cap 65536 blocks, 1024 iterations per block.");
    for (c = 0; c < CPU_CASE_COUNT; c++) {
        U32 bad = 0;
        line(cpu_case_names[c]);
        number("CASE_ID=", c);
        number("runtime_calls_per_iteration=", cpu_calls_per_iteration[c]);
        number("expected_checksum=", cpu_expected[c]);
        hardware_diag_progress(cpu_case_names[c], 2);
        value = cpu_case_block(c);
        if (value != cpu_expected[c]) {
            number("actual_checksum=", value);
            line("CASE_RESULT FAIL warmup checksum; score suppressed");
            failed = 1;
            hardware_diag_progress(cpu_case_names[c], 0);
            continue;
        }
        for (pass = 0; pass < 3; pass++) {
            blocks = 0;
            rates[pass] = 0;
            start = GetTickCount();
            do {
                value = cpu_case_block(c);
                blocks++;
                if (value != cpu_expected[c]) { bad = 1; break; }
                elapsed = GetTickCount() - start;
            } while (elapsed < CPU_CASE_MS && blocks < CPU_CASE_MAX_BLOCKS);
            if (bad) { line("FAIL timed checksum"); break; }
            if (!elapsed || elapsed >= 0x80000000u) {
                line("FAIL clock did not advance or moved backwards"); bad = 1; break;
            }
            number("case_pass=", pass + 1);
            number("case_elapsed_ms=", elapsed);
            number("case_work_iterations=", blocks * CPU_CASE_ITERATIONS);
            number("case_sample_capped=", blocks == CPU_CASE_MAX_BLOCKS && elapsed < CPU_CASE_MS);
            /* Quotient/remainder decomposition avoids a 64-bit timing divide.
               Check the second product as well, for broken/extreme clocks. */
            {
                U32 iterations = blocks * CPU_CASE_ITERATIONS;
                U32 q = __rt_udiv(elapsed, iterations), rem = iterations - q * elapsed;
                if (q > 4294966u || rem > 4294967u) { bad = 1; break; }
                rates[pass] = q * 1000u + __rt_udiv(elapsed, rem * 1000u);
                if (!rates[pass]) { bad = 1; break; }
            }
            number("case_iterations_per_second=", rates[pass]);
        }
        if (bad) {
            failed = 1;
            line("CASE_RESULT FAIL; score suppressed");
        } else {
            if (rates[0] > rates[1]) { tmp = rates[0]; rates[0] = rates[1]; rates[1] = tmp; }
            if (rates[1] > rates[2]) { tmp = rates[1]; rates[1] = rates[2]; rates[2] = tmp; }
            if (rates[0] > rates[1]) { tmp = rates[0]; rates[0] = rates[1]; rates[1] = tmp; }
            number("CASE_MEDIAN_ITERATIONS_PER_SECOND=", rates[1]);
            line("CASE_RESULT PASS");
        }
        hardware_diag_progress(cpu_case_names[c], !bad);
    }
    line(failed ? "CPU_COST_PROBES_RESULT FAIL" : "CPU_COST_PROBES_RESULT PASS");
    return failed;
}
