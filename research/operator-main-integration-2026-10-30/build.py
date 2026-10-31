import subprocess,pathlib,gzip,json,hashlib
p=pathlib.Path(__file__).resolve().parent
cmd=["rtk","proxy","cargo","build","--features","gui,acceptance-diagnostics","--bin","ovrcr","--bin","ovrcr-gui"]
c=subprocess.run(cmd,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
(p/"gui-cli-build.log.gz").write_bytes(gzip.compress(c.stdout,mtime=0))
(p/"gui-cli-build.json").write_text(json.dumps({"command":cmd,"exit":c.returncode,"raw_bytes":len(c.stdout),"sha256":hashlib.sha256(c.stdout).hexdigest()},indent=2)+"\n")
print(c.stdout.decode());raise SystemExit(c.returncode)
