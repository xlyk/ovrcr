import gzip,hashlib,json,os,pathlib,subprocess,tempfile,time,sys
repo=pathlib.Path(__file__).resolve().parents[2]; ev=pathlib.Path(__file__).resolve().parent
root=pathlib.Path(tempfile.mkdtemp(prefix="ovrcr-journal-gates-",dir="/private/tmp")); root.chmod(0o700)
for n in ["home","tmp","config","cache","instance"]:(root/n).mkdir(mode=0o700)
env={"PATH":os.environ["PATH"],"HOME":str(root/"home"),"TMPDIR":str(root/"tmp"),"XDG_CONFIG_HOME":str(root/"config"),"XDG_CACHE_HOME":str(root/"cache"),"OVRCR_HOME":str(root/"instance"),"OVRCR_SOCKET":str(root/"instance/server.sock"),"RUSTUP_HOME":"/Users/xlyk/.rustup","CARGO_HOME":"/Users/xlyk/.cargo","SHELL":"/bin/zsh","NO_PROXY":"*","no_proxy":"*","GIT_CONFIG_NOSYSTEM":"1"}
(ev/("quick-corrected-environment.json" if len(sys.argv)>1 else "gates-environment.json")).write_text(json.dumps({"root":str(root),"env":env,"source_revision":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip()},indent=2)+"\n")
commands=[("compiled-worker",["cargo","test","-p","ovrcr","--test","operator_e2e","--features","gui,acceptance-diagnostics","compiled_worker","--","--nocapture"]),("quick-outer-pty-corrected",["cargo","test","-p","ovrcr","--test","terminal_acceptance","operator_quick","--","--nocapture"]),("workspace-test",["cargo","test","--workspace","--all-targets","--all-features"]),("workspace-clippy",["cargo","clippy","--workspace","--all-targets","--all-features","--","-D","warnings"]),("workspace-check",["cargo","check","--workspace","--all-targets","--all-features"]),("fmt",["cargo","fmt","--all","--","--check"]),("diff",["git","diff","--check"])]
if len(sys.argv)>1: commands=[row for row in commands if row[0]=="quick-outer-pty-corrected"]
for name,cmd in commands:
 start=time.time(); digest=hashlib.sha256(); size=0
 with (ev/(name+".log.gz")).open("wb") as raw,gzip.GzipFile(filename="",mode="wb",fileobj=raw,mtime=0) as log:
  child=subprocess.Popen(cmd,cwd=repo,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  (ev/(name+"-owner.json")).write_text(json.dumps({"pid":child.pid,"command":cmd},indent=2)+"\n")
  while chunk:=child.stdout.read1(65536):digest.update(chunk);size+=len(chunk);log.write(chunk);log.flush()
  result=child.wait()
 receipt={"command":cmd,"exit":result,"seconds":time.time()-start,"raw_bytes":size,"raw_sha256":digest.hexdigest(),"stored_sha256":hashlib.sha256((ev/(name+".log.gz")).read_bytes()).hexdigest()}
 (ev/(name+"-result.json")).write_text(json.dumps(receipt,indent=2)+"\n");print(name,"original wait",result,flush=True)
print("GATES_QUIESCENT",str(root),flush=True)
