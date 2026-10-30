import os,sys,subprocess,pathlib,json,datetime,tempfile,shutil
root=pathlib.Path.cwd()/"research/operator-layout-2026-10-30"
name=sys.argv[1]; command=sys.argv[2:]
revision=subprocess.check_output(["rtk","git","rev-parse","HEAD"],text=True).strip()
private=pathlib.Path(tempfile.mkdtemp(prefix="operator-layout-gate-",dir="/private/tmp"))
(private/"home").mkdir();(private/"temp").mkdir();(private/"gitconfig").write_text("[user]\nname = OVRCR Fixture\nemail = fixture@ovrcr.invalid\n")
environment={"PATH":os.environ["PATH"],"HOME":str(private/"home"),"TMPDIR":str(private/"temp"),"CARGO_HOME":"/Users/xlyk/.cargo","RUSTUP_HOME":"/Users/xlyk/.rustup","CARGO_NET_OFFLINE":"true","SHELL":"/bin/zsh","NO_PROXY":"*","no_proxy":"*","GIT_CONFIG_GLOBAL":str(private/"gitconfig"),"GIT_CONFIG_NOSYSTEM":"1"}
with (root/(name+".log")).open("w") as out:
 result=subprocess.run(command,stdout=out,stderr=subprocess.STDOUT,env=environment)
(root/(name+".json")).write_text(json.dumps({"command":command,"exit":result.returncode,"revision":revision,"private_root":str(private),"isolated_environment_keys":sorted(environment),"utc":datetime.datetime.now(datetime.timezone.utc).isoformat()},indent=2)+"\n")
print(name,"exit",result.returncode,flush=True)
if result.returncode==0: shutil.rmtree(private)
print((root/(name+".log")).read_text()[-6000:],flush=True)
sys.exit(result.returncode)
