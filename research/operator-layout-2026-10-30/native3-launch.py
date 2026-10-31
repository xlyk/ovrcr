import os,sys,json,pathlib,subprocess,time
repo=pathlib.Path.cwd();e=repo/"research/operator-layout-2026-10-30";s=json.loads((e/"native3-state.json").read_text());r=pathlib.Path(s["root"]);phase=sys.argv[1]
env={"PATH":"/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin","HOME":str(r/"home"),"TMPDIR":str(r/"temp"),"SHELL":"/bin/zsh","NO_PROXY":"*","no_proxy":"*"}
if phase=="holder":
 env.update(OVRCR_OPERATOR_E2E_HOLDER=str(r/"control"),OVRCR_OPERATOR_E2E_DEMO_PARENT=str(r/"temp"));cmd=[s["holder_executable"],"--exact","compiled_worker::native_provider_holder","--ignored","--nocapture","--test-threads=1"]
else:
 env["OVRCR_OPERATOR_TEST_API_BASE"]=json.loads((r/"control/base.json").read_text())["api_base"];cmd=[str(pathlib.Path(s["bundle"])/"Contents/MacOS/ovrcr-gui")]
with (e/("native3-"+phase+".log")).open("w") as out:
 child=subprocess.Popen(cmd,env=env,stdout=out,stderr=subprocess.STDOUT)
 (e/("native3-"+phase+"-owner.json")).write_text(json.dumps({"pid":child.pid,"command":cmd,"root":str(r),"revision":s["revision"]},indent=2)+"\n")
 print(phase,"pid",child.pid,flush=True);exit=child.wait()
(e/("native3-"+phase+"-exit.json")).write_text(json.dumps({"original_wait_exit":exit},indent=2)+"\n");print(phase,"original exit",exit,flush=True);sys.exit(exit)
