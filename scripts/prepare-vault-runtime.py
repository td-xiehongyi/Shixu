#!/usr/bin/env python3
"""Explicit public dependency preparation; never called by application startup.
Fixed official archives, npm integrity lock, no install scripts. Writes only repo
resources and task-owned user-temp cache. Manifest must match committed reviewed lock.
"""
import argparse, hashlib, json, os, shutil, subprocess, tarfile, tempfile, urllib.request, zipfile
from pathlib import Path
ROOT = Path(__file__).resolve().parent.parent
CACHE = Path(tempfile.gettempdir()) / 'shixu-vault-runtime-downloads'
PINS = {
 'linux-x64': ('node-v26.11.1-linux-x64.tar.xz','3883bfc73f9a680ca4eab04b196068aaaab1373ffa77d8fc1a4408222495b651'),
 'win-x64': ('node-v26.11.1-win-x64.zip','97f36a8a9684ff0d3e35758b4610fef5b628a5880e96f2ccc11240e5daf9934e'),
}
def sha(file): return hashlib.sha256(file.read_bytes()).hexdigest()
def prepare(platform, update, offline=False):
 name, digest = PINS[platform]
 CACHE.mkdir(parents=True, exist_ok=True)
 archive = CACHE / name
 if not archive.exists() and offline: raise RuntimeError('BLOCKED: pinned runtime archive missing from explicit user-temp cache')
 if not archive.exists(): urllib.request.urlretrieve('https://nodejs.org/dist/v26.11.1/'+name,archive)
 if sha(archive) != digest: raise RuntimeError('runtime archive hash mismatch')
 target = ROOT / 'resources' / ('vault-'+platform)
 shutil.rmtree(target, ignore_errors=True)
 runtime = target / 'runtime'; runtime.mkdir(parents=True)
 prefix = name.removesuffix('.tar.xz').removesuffix('.zip')+'/'
 if platform == 'linux-x64':
  with tarfile.open(archive) as src:
   for member, dest in [('bin/node','node'),('LICENSE','LICENSE')]:
    (runtime/dest).write_bytes(src.extractfile(prefix+member).read())
  (runtime/'node').chmod(0o755)
 else:
  with zipfile.ZipFile(archive) as src:
   for member in ['node.exe','LICENSE']: (runtime/member).write_bytes(src.read(prefix+member))
 helper = target / 'helper'; helper.mkdir()
 for file in ['helper.mjs','crypto.mjs','package.json','package-lock.json']:
  shutil.copy2(ROOT/'vault-helper'/file, helper/file)
 shutil.copytree(ROOT/'vault-helper/kdbx-lib',helper/'kdbx-lib')
 for dep in ['@xmldom/xmldom','fflate','hash-wasm']:
  shutil.copytree(ROOT/'vault-helper/node_modules'/dep,helper/'node_modules'/dep)
 shutil.copy2(ROOT/'vault-helper/node_modules/kdbxweb/LICENSE',helper/'KdbxWeb-LICENSE')
 files={file.relative_to(target).as_posix():sha(file) for file in sorted(target.rglob('*')) if file.is_file()}
 lock=ROOT/'vault-helper'/('resources-'+platform+'.json')
 manifest={'version':1,'node':'26.11.1','kdbxweb':'2.1.1','hash_wasm':'4.12.0','archive':name,'archive_sha256':digest,'files':files}
 if update: lock.write_text(json.dumps(manifest,indent=2)+'\n')
 elif json.loads(lock.read_text()) != manifest: raise RuntimeError('prepared resources differ from reviewed manifest')
 print(platform, 'verified files:',len(files),'binary sha256:',files['runtime/node' if platform=='linux-x64' else 'runtime/node.exe'])
def verify(platform):
 name, digest = PINS[platform]
 target = ROOT / 'resources' / ('vault-'+platform)
 paths = list(target.rglob('*'))
 if not target.is_dir() or target.is_symlink() or any(file.is_symlink() or getattr(file.lstat(), 'st_file_attributes', 0) & 0x400 for file in [target, *paths]):
  raise RuntimeError('prepared resource missing or reparse')
 files={file.relative_to(target).as_posix():sha(file) for file in sorted(paths) if file.is_file() and file.stat().st_size <= 256*1024*1024}
 manifest={'version':1,'node':'26.11.1','kdbxweb':'2.1.1','hash_wasm':'4.12.0','archive':name,'archive_sha256':digest,'files':files}
 if len(files) != 198 or json.loads((ROOT/'vault-helper'/('resources-'+platform+'.json')).read_text()) != manifest:
  raise RuntimeError('prepared resources differ from reviewed manifest')
 print(platform, 'verified files:',len(files),'binary sha256:',files['runtime/node' if platform=='linux-x64' else 'runtime/node.exe'])
if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('--platform',choices=['linux-x64','win-x64','both'],default='both');parser.add_argument('--update-reviewed-manifest',action='store_true');parser.add_argument('--verify-only',action='store_true');parser.add_argument('--offline',action='store_true');args=parser.parse_args()
 if args.verify_only and args.update_reviewed_manifest: parser.error('verify-only cannot update reviewed manifest')
 if not args.verify_only:
  if args.offline:
   for platform in (PINS if args.platform=='both' else [args.platform]):
    name, digest = PINS[platform]
    archive = CACHE / name
    if not archive.is_file() or sha(archive) != digest: raise RuntimeError('BLOCKED: pinned runtime archive cache missing or mismatched; explicit preparation required first')
  npm = shutil.which('npm.cmd' if os.name == 'nt' else 'npm')
  node = shutil.which('node')
  if not npm or not node: raise RuntimeError('explicit preparation requires installed npm and Node build tools')
  subprocess.run([npm,'ci','--ignore-scripts',*(['--offline'] if args.offline else []),'--cache',str(CACHE/'npm-cache'),'--prefix',str(ROOT/'vault-helper')],check=True)
  subprocess.run([node,str(ROOT/'vault-helper/build.cjs')],check=True)
 for platform in (PINS if args.platform=='both' else [args.platform]):
  if args.verify_only: verify(platform)
  else: prepare(platform,args.update_reviewed_manifest,args.offline)
