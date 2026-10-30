import pathlib,subprocess,gzip,json,hashlib,tempfile,shutil
repo=pathlib.Path(__file__).resolve().parents[2]; ev=pathlib.Path(__file__).resolve().parent
cmd=["rtk","proxy","cargo","test","-p","ovrcr","--test","operator_e2e","--features","gui,acceptance-diagnostics","--no-run","--message-format=json"]
child=subprocess.Popen(cmd,cwd=repo,stdout=subprocess.PIPE,stderr=subprocess.STDOUT); raw=child.communicate()[0];result=child.returncode
(ev/"holder-build.log.gz").write_bytes(gzip.compress(raw,mtime=0));(ev/"holder-build-result.json").write_text(json.dumps({"command":cmd,"exit":result,"raw_bytes":len(raw),"raw_sha256":hashlib.sha256(raw).hexdigest()},indent=2)+"\n");assert result==0
artifacts=[json.loads(l) for l in raw.decode().splitlines() if l.startswith("{")];holder=next(a["executable"] for a in artifacts if a.get("reason")=="compiler-artifact" and a.get("target",{}).get("name")=="operator_e2e")
root=pathlib.Path(tempfile.mkdtemp(prefix="ovrcr-main-native-",dir="/private/tmp"));root.chmod(0o700)
for name in ["home","temp","control"]:(root/name).mkdir(mode=0o700)
(root/"gitconfig").write_text("[user]\nname = OVRCR Fixture\nemail = fixture@ovrcr.invalid\n")
bundle=repo/"target/OVRCR GUI.app"
for name in ["ovrcr","ovrcr-gui"]:
 target=bundle/"Contents/MacOS"/name;target.unlink(missing_ok=True);shutil.copy2(repo/"target/debug"/name,target)
shutil.copy2(holder,root/"operator_e2e")
state={"source_revision":subprocess.check_output(["rtk","proxy","git","rev-parse","HEAD"],cwd=repo,text=True).strip(),"root":str(root),"bundle":str(bundle),"holder":str(root/"operator_e2e"),"features":["gui","acceptance-diagnostics"],"sha256":{name:hashlib.sha256((bundle/"Contents/MacOS"/name).read_bytes()).hexdigest() for name in ["ovrcr","ovrcr-gui"]},"holder_sha256":hashlib.sha256((root/"operator_e2e").read_bytes()).hexdigest()}
state["source_sha256"]={str(path.relative_to(repo)):hashlib.sha256(path.read_bytes()).hexdigest() for path in [repo/"crates/ovrcr-tui/src/dashboard/operator.rs",repo/"crates/ovrcr-tui/src/dashboard/operator/render.rs",repo/"crates/ovrcr-tui/src/dashboard/render.rs",repo/"crates/ovrcr-tui/src/dashboard/state.rs",repo/"crates/ovrcr-tui/src/dashboard/text_cursor.rs",repo/"src/gui/input.rs",repo/"tests/operator_e2e.rs",repo/"tests/support/operator_provider.rs"]}
(ev/"state.json").write_text(json.dumps(state,indent=2)+"\n"); print(json.dumps(state,indent=2))
