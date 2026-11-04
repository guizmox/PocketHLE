from pathlib import Path
import tomllib,subprocess,os
r=Path('gpu-x1-work').resolve();out=Path('tmp/deferred-gapi-build').resolve();out.mkdir(exist_ok=True);deps=Path('build-small/debug/deps').resolve();cached=Path('tmp/x1-app-build').resolve();env=os.environ.copy();env['LD_LIBRARY_PATH']=str(Path('toolchain/direct/lib').resolve());env['CARGO_PKG_VERSION']='0.4.1'
for pkg in ['winceapi','core']:
 source=r/'crates'/('pocket-'+pkg)
 cargo=tomllib.loads((r/'crates'/('pocket-'+pkg)/'Cargo.toml').read_text());name='pocket_'+pkg
 args=[str(Path('toolchain/direct/bin/rustc').resolve()),'--edition=2021','--crate-name',name,'--crate-type=rlib',str(source/'src/lib.rs'),'-C','opt-level=0','-C','debuginfo=0','-L','dependency='+str(out),'-L','dependency='+str(cached),'-L','dependency='+str(deps),'-o',str(out/('lib'+name+'.rlib'))]
 for dep,cfg in cargo['dependencies'].items():
  if isinstance(cfg,dict) and cfg.get('optional',False) and not(pkg=='cpu' and dep=='unicorn-engine'):continue
  n=dep.replace('-','_');paths=[out/('lib'+n+'.rlib'),cached/('lib'+n+'.rlib')]
  paths=[p for p in paths if p.exists()] if n.startswith('pocket_') else []
  if not paths:paths=sorted(deps.glob('lib'+n+'-*.rlib'))
  assert paths,n
  args+=['--extern',n+'='+str(paths[0])]
 if pkg in ['cpu','core']:args+=['--cfg','feature="unicorn"']
 if pkg=='kernel':args+=['--extern','libc='+str(sorted(deps.glob('liblibc-*.rlib'))[0])]
 env['CARGO_MANIFEST_DIR']=str(source);print('BUILD',pkg,flush=True)
 with (out/(pkg+'-build.log')).open('w') as f:p=subprocess.run(args,env=env,stdout=f,stderr=f)
 if p.returncode:print((out/(pkg+'-build.log')).read_text()[-3000:]);raise SystemExit(p.returncode)
