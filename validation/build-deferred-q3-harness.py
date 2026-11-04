from pathlib import Path
import tomllib,subprocess,os
r=Path('gpu-x1-work').resolve();out=Path('tmp/deferred-gapi-build').resolve();deps=Path('build-small/debug/deps').resolve();cached=Path('tmp/x1-app-build').resolve();env=os.environ.copy();env['LD_LIBRARY_PATH']=str(Path('toolchain/direct/lib').resolve());env['CARGO_PKG_VERSION']='0.4.1'
args=[str(Path('toolchain/direct/bin/rustc').resolve()),'--edition=2021',str(out/'q3-run.rs'),'-C','linker=/usr/bin/gcc','-C','link-arg=-fuse-ld=bfd','-L','native='+str(Path('tmp/unicorn-current-build').resolve()),'-L','dependency='+str(out),'-L','dependency='+str(cached),'-L','dependency='+str(deps),'--extern','pocket_gles='+str(cached/'libpocket_gles.rlib'),'--extern','pocket_core='+str(out/'libpocket_core.rlib'),'--extern','anyhow='+str(sorted(deps.glob('libanyhow-*.rlib'))[0]),'--extern','log='+str(sorted(deps.glob('liblog-*.rlib'))[0]),'-o',str(out/'q3-shared-probe')]
with (out/'run-build.log').open('w') as f:p=subprocess.run(args,env=env,stdout=f,stderr=f)
print('HARNESS',p.returncode)
if p.returncode:print((out/'run-build.log').read_text());raise SystemExit(p.returncode)
