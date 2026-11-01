"""Controlled ARM execution of CPU v2 probes, medians and failure paths.

Executes the real integrated EXE, using host arithmetic as the WinCE API shim.
It does not claim validation of PocketHLE's actual runtime handlers or timing.
"""
import re
import guest_smoke as base
import cpu_benchmark_reference as ref


class CostGuest(base.Guest):
    def __init__(self, mode, corrupt=None, bad_clock=False):
        super().__init__(base.root / 'AUTORUN.EXE')
        self.command = '--hd-' + mode
        self.report_path = '\\Flash Disk\\' + ('INTEGER.TXT' if mode == 'integer' else 'FLOATING.TXT')
        self.corrupt = corrupt
        self.bad_clock = bad_clock
        self.case_text = ''
        self.clock_reads = 0
        self.files['\\Flash Disk\\HARDWARELIVE.TXT'] = bytearray(b' ' * 240)

    def hook(self, u, a, size, data):
        name = self.apis.get(a)
        if name == 'GetTickCount':
            if self.case_text:
                if self.bad_clock:
                    value = 0 if self.clock_reads % 2 == 0 else 0x80000010
                    self.clock_reads += 1
                else:
                    count = len(re.findall(r'^case_pass=', self.case_text, re.M))
                    self.time += [123, 211, 307][min(count, 2)]
                    value = self.time
            else:
                self.time += 250
                value = self.time
            u.reg_write(base.UC_ARM_REG_R0, value)
            u.reg_write(base.UC_ARM_REG_PC, u.reg_read(base.UC_ARM_REG_LR))
            return
        super().hook(u, a, size, data)
        if name == self.corrupt:
            # Corrupt ONLY the upper return word for the 64-bit multiply probe.
            reg = base.UC_ARM_REG_R1 if name == '__ll_mul' else base.UC_ARM_REG_R0
            u.reg_write(reg, u.reg_read(reg) ^ 1)
        if name == 'WriteFile':
            text = bytes(self.files.get(self.report_path, b'')).decode()
            if '\r\nCASE_ID=' in text:
                self.case_text = text.split('\r\nCASE_ID=')[-1]

    def report(self):
        self.run()
        assert hasattr(self, 'exit'), 'ARM test did not finish'
        assert not self.created, 'ordinary benchmark must stay inside the one EXE'
        assert len(self.files['\\Flash Disk\\HARDWARELIVE.TXT']) == 240
        return bytes(self.files[self.report_path]).decode()


for mode, names in [('integer', ref.INT_CASES), ('floating', ref.FLOAT_CASES)]:
    g = CostGuest(mode)
    text = g.report()
    assert g.exit == 0 and 'CPU_COST_PROBES_RESULT PASS' in text, text
    assert len(re.findall(r'^elapsed_ms=', text, re.M)) == 3, 'legacy samples must stay separate'
    for i, name in enumerate(names):
        section = text.split(name + '\r\nCASE_ID=' + str(i) + '\r\n', 1)[1]
        section = section.split('CASE_RESULT', 1)[0]
        expected = int(re.search(r'^expected_checksum=(\d+)', section, re.M)[1])
        assert expected == ref.checksum(name), name
        rates = list(map(int, re.findall(r'^case_iterations_per_second=(\d+)', section, re.M)))
        median = int(re.search(r'^CASE_MEDIAN_ITERATIONS_PER_SECOND=(\d+)', section, re.M)[1])
        assert len(rates) == 3 and len(set(rates)) == 3 and median == sorted(rates)[1], (name, rates)
    assert text.count('CASE_RESULT PASS') == len(names)
    print(mode, 'ARM v2: all cases, independent checksums, separate legacy score, 3-pass medians PASS')

for mode, api, bad_name in [('integer', '__ll_mul', 'u64_multiply_runtime'),
                            ('floating', '__divs', 'f32_divide_runtime'),
                            ('integer', '__rt_sdiv', 'series_i32_signed_divide'),
                            ('floating', '__lts', 'series_f32_runtime')]:
    g = CostGuest(mode, corrupt=api)
    text = g.report()
    assert g.exit == 1 and 'CPU_COST_PROBES_RESULT FAIL' in text, text
    section = text.split(bad_name + '\r\n', 1)[1].split('CASE_RESULT', 1)
    assert 'CASE_MEDIAN_ITERATIONS_PER_SECOND=' not in section[0]
    assert section[1].startswith(' FAIL warmup checksum')
    assert 'FINAL_' in text and 'FINAL_SCORE INVALID' not in text, 'valid historical result must remain available'
    print(mode, 'ARM v2: corrupt result rejected, affected case score suppressed PASS')

g = CostGuest('integer', bad_clock=True)
text = g.report()
assert g.exit == 1 and 'CPU_COST_PROBES_RESULT FAIL' in text
assert 'CASE_MEDIAN_ITERATIONS_PER_SECOND=' not in text
print('ARM v2: invalid/backward clock suppresses all new scores PASS')
