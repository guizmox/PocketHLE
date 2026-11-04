from pathlib import Path
import tomllib,os,subprocess,sys
r=Path('gpu-x1-work').resolve();out=Path('tmp/deferred-gapi-build').resolve();deps=Path('build-small/debug/deps').resolve();env=os.environ.copy();env['LD_LIBRARY_PATH']=str(Path('toolchain/direct/lib').resolve());env['CARGO_PKG_VERSION']='0.4.1';rust=str(Path('toolchain/direct/bin/rustc').resolve())
out.mkdir(exist_ok=True)
for pkg in ['kernel']:
 cargo=tomllib.loads((r/'crates'/('pocket-'+pkg)/'Cargo.toml').read_text());name='pocket_'+pkg
 a=[rust,'--edition=2021','--crate-name',name,'--test','-C','linker=/usr/bin/gcc','-C','link-arg=-fuse-ld=bfd','-L','native='+str(Path('tmp/unicorn-current-build').resolve()),str(r/'crates'/('pocket-'+pkg)/'src/lib.rs'),'-C','debuginfo=0','-C','opt-level=0','-L','dependency='+str(deps),'-L','dependency='+str(out),'-o',str(out/'kernel-tests')]
 for dep,cfg in cargo['dependencies'].items():
  if isinstance(cfg,dict) and cfg.get('optional',False):continue
  k=dep.replace('-','_');p=out/('lib'+k+'.rlib')
  if not k.startswith('pocket_') or not p.exists():
   paths=sorted(deps.glob('lib'+k+'-*.rlib'))
   if not paths:raise RuntimeError(k)
   p=paths[0]
  a+=['--extern',k+'='+str(p)]
 if pkg in ('cpu','core'):a+=['--cfg','feature="unicorn"']
 if pkg=='cpu':a+=['--extern','unicorn_engine='+str(sorted(deps.glob('libunicorn_engine-*.rlib'))[0])]
 if pkg=='kernel':a+=['--extern','libc='+str(sorted(deps.glob('liblibc-*.rlib'))[0])]
 a+=['--extern','tempfile='+str(sorted(deps.glob('libtempfile-*.rlib'))[0])]
 env['CARGO_MANIFEST_DIR']=str(r/'crates'/('pocket-'+pkg));print('BUILD',pkg,flush=True)
 with (out/(pkg+'-build.log')).open('w') as f:p=subprocess.run(a,env=env,stdout=f,stderr=f)
 if p.returncode:print((out/(pkg+'-build.log')).read_text()[-5000:]);raise SystemExit(p.returncode)
