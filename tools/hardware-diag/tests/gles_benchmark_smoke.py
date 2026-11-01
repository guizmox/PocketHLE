"""Execute the actual ARM benchmark with deterministic GL/clock shims.

No real GPU or PocketHLE API validation is implied. Existing integration smoke
checks run first because guest_smoke supplies the shared ARM harness.
"""
import math
import re
import struct
from collections import Counter
import guest_smoke as base

CASES = ["geometry_512", "geometry_2048", "geometry_8192", "draw_calls_128",
         "matrix_animated", "vertices_rebuilt", "fill_solid",
         "fill_texture_nearest", "fill_texture_linear", "fill_texture_blend"]


class GLGuest(base.Guest):
    def __init__(self, **options):
        super().__init__(base.root / 'AUTORUN.EXE')
        self.options = options
        self.command = '--hd-glesbench'
        self.case = 0
        self.sample = 0
        self.glcalls = Counter()
        self.triangle_counts = Counter()
        self.draw_counts = Counter()
        self.frames = Counter()
        self.swaps = Counter()
        self.pointer = 0
        self.current_draws = self.current_triangles = 0
        self.files['\\Flash Disk\\HARDWARELIVE.TXT'] = bytearray(b' ' * 240)

    def cstring(self, p):
        b = bytearray()
        while self.u.mem_read(p, 1) != b'\0':
            b.extend(self.u.mem_read(p, 1))
            p += 1
        return b.decode()

    def hook(self, u, a, size, data):
        name = self.apis.get(a)
        value = 1
        if name == 'LoadLibraryW':
            value = 10
        elif name == 'FreeLibrary':
            self.calls[name] += 1
        elif name == 'GetProcAddressA':
            api = self.cstring(self.arg(1))
            if api == self.options.get('missing'):
                value = 0
            else:
                value = 0x70008000 + len(self.glcalls) * 32
                self.glcalls[api] = 0
                self.apis[value] = api
        elif name == 'GetTickCount':
            value = 0 if self.options.get('stalled') else self.time
        elif name and name.startswith(('egl', 'gl')):
            self.glcalls[name] += 1
            if name == 'eglInitialize':
                self.put(self.arg(1), 1)
                self.put(self.arg(2), 1)
            elif name == 'eglChooseConfig':
                self.put(self.arg(2), 20)
                self.put(self.arg(4), 1)
            elif name == 'eglSwapBuffers':
                self.swaps[self.case] += 1
            elif name == 'glGenTextures':
                self.put(self.arg(1), 30)
            elif name == 'glGetString':
                u.mem_write(0x7000b000, b'Controlled ARM GL shim\0')
                value = 0x7000b000
            elif name == 'glGetError':
                value = 0x500 if self.options.get('gl_error') else 0
            elif name == 'glReadPixels':
                assert (self.arg(0), self.arg(1)) in ((160, 120), (0, 0))
                u.mem_write(self.arg(6), bytes(4) if self.options.get('black') else bytes([255, 120, 60, 255]))
            elif name == 'glVertexPointer':
                assert self.arg(0) == 2 and self.arg(1) == 0x140c
                self.pointer = self.arg(3)
            elif name == 'glDrawArrays':
                first, count = self.arg(1), self.arg(2)
                assert self.arg(0) == 4 and count % 3 == 0
                coords = struct.unpack('<' + 'i' * count * 2,
                                      u.mem_read(self.pointer + first * 8, count * 8))
                assert all(-65536 <= x <= 65536 for x in coords), (self.case, min(coords), max(coords))
                self.current_draws += 1
                self.current_triangles += count // 3
                expected = [512, 2048, 8192, 512, 512, 512, 32, 32, 32, 32][self.case]
                if self.current_triangles < expected:
                    u.reg_write(base.UC_ARM_REG_R0, value)
                    u.reg_write(base.UC_ARM_REG_PC, u.reg_read(base.UC_ARM_REG_LR))
                    return
                expected = [512, 2048, 8192, 512, 512, 512, 32, 32, 32, 32][self.case]
                draws = 16 if self.case >= 6 else 128 if self.case == 3 else 1
                assert (self.current_triangles, self.current_draws) == (expected, draws)
                self.current_draws = self.current_triangles = 0
                self.frames[self.case] += 1
                self.time += 12 + self.case * 2 + self.sample * 3
        else:
            super().hook(u, a, size, data)
            if name == 'WriteFile':
                path = '\\Flash Disk\\OGLESBENCH.TXT'
                text = bytes(self.files.get(path, b'')).decode()
                tests = re.findall(r'^TEST=([^\r\n]+)\r\n', text, re.M)
                if tests:
                    case = CASES.index(tests[-1])
                    if case != self.case:
                        self.case, self.sample = case, 0
                    self.sample = len(re.findall(r'^render_millifps=', text.split('TEST=')[-1], re.M))
            return
        u.reg_write(base.UC_ARM_REG_R0, value)
        u.reg_write(base.UC_ARM_REG_PC, u.reg_read(base.UC_ARM_REG_LR))


def run(**options):
    g = GLGuest(**options)
    g.run()
    assert hasattr(g, 'exit'), 'ARM execution did not complete'
    report = bytes(g.files['\\Flash Disk\\OGLESBENCH.TXT']).decode()
    assert not g.created, 'benchmark must stay inside the same EXE'
    assert len(g.files['\\Flash Disk\\HARDWARELIVE.TXT']) == 240
    return g, report


g, report = run()
assert g.exit == 0 and 'OGLESBENCH_RESULT PASS' in report, report
sections = report.split('TEST=')[1:]
assert len(sections) == 10
medians = []
for i, section in enumerate(sections):
    assert section.startswith(CASES[i] + '\r\n')
    rates = list(map(int, re.findall(r'^render_millifps=(\d+)', section, re.M)))
    median = int(re.search(r'^MEDIAN_RENDER_MILLIFPS=(\d+)', section, re.M)[1])
    assert len(rates) == 3 and median == sorted(rates)[1], section
    medians.append(median)
    assert g.frames[i] > 12 and g.swaps[i] == 12, (i, g.frames[i], g.swaps[i])
score = int(re.search(r'^GLOBAL_INDEX_MILLI=(\d+)', report, re.M)[1])
reference = math.exp(sum(math.log(x) for x in medians) / 10)
assert abs(score - reference) / reference < .001, (score, reference)
assert g.glcalls['glRotatex'] > 0 and g.glcalls['glFinish'] >= 40
print('OpenGLES ARM: all 10 workloads, 3 samples, medians, geometric global, cleanup PASS')
for options in [dict(missing='glFinish'), dict(gl_error=True), dict(black=True), dict(stalled=True)]:
    g, report = run(**options)
    assert g.exit == 1 and 'OGLESBENCH_RESULT FAIL' in report, report
    assert 'GLOBAL_INDEX_MILLI=' not in report
    assert g.calls['FreeLibrary'] == 1
    if not options.get('missing'):
        assert g.glcalls['eglDestroyContext'] == g.glcalls['eglDestroySurface'] == g.glcalls['eglTerminate'] == 1
    print('OpenGLES ARM: expected failure suppresses score and cleans up', options)

class BenchmarkMenuGuest(base.Guest):
    def hook(self, u, a, size, data):
        if self.apis.get(a) == 'Sleep':
            self.sleeps += 1
            if self.sleeps == 1:
                self.messages = [40] * 11 + [13]
            if self.sleeps == 8:
                u.emu_stop()
            u.reg_write(base.UC_ARM_REG_R0, 0)
            u.reg_write(base.UC_ARM_REG_PC, u.reg_read(base.UC_ARM_REG_LR))
            return
        super().hook(u, a, size, data)

menu = BenchmarkMenuGuest(base.root / 'AUTORUN.EXE')
menu.run()
assert 'OpenGLES Benchmark' in menu.text
assert len(menu.created) == 1 and menu.created[0][0] == 'thread' and menu.created[0][2] == 11
assert not menu.calls['CreateProcessW']
print('OpenGLES ARM: controller navigation selects the new same-process benchmark entry PASS')
