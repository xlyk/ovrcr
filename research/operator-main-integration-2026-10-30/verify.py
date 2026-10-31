"""Offline validation of saved native evidence; does not open a socket or app."""
import gzip, hashlib, json, pathlib
p=pathlib.Path(__file__).resolve().parent
repo=p.parents[1]
read=lambda name:json.loads((p/(name+".json")).read_text())
state=read("state")
assert state["source_revision"]=="ffebfe859b7aae83872354ef55b12edd63db484e"
for name in ["gui-cli-build","holder-build-result"]:assert read(name)["exit"]==0
protocol=read("protocol-schema")
assert protocol["pragma_user_version"]==27
assert protocol["handshake"]["server_protocol"]==70
assert protocol["handshake"]["sent_hex"]==protocol["handshake"]["received_hex"]=="4f56524300000046"
assert protocol["normal_cli"]["exit"]==0
assert hashlib.sha256((p/"cli-list.json").read_bytes()).hexdigest()==protocol["normal_cli"]["stdout_sha256"]
texts={}
for path in sorted(p.glob("[0-9][0-9]-*.jpg")):
 name=path.stem;meta=read(name);raw=path.read_bytes();text=gzip.decompress((p/(name+".ax.txt.gz")).read_bytes())
 assert raw[:3]==b"\xff\xd8\xff"
 assert hashlib.sha256(raw).hexdigest()==meta["jpeg_sha256"]
 assert hashlib.sha256(text).hexdigest()==meta["ax_sha256"]
 texts[name]=text.decode()
assert len(texts)==7
for marker in ["Responding","succeeded","Full ▏preserved 界🙂 tail","second café"]:assert marker in texts["02-responding-tools-retained-draft"]
for marker in ["Quick ▏preserved 界🙂 tail","Full ▏preserved 界🙂 tail","second café"]:assert marker in texts["03-quick-common-input"]
assert "running" in texts["04-cleared-still-running"] and "Full ▏preserved" not in texts["04-cleared-still-running"]
assert "interrupted" in texts["05-separate-selected-stop"]
assert "Unarchive: Operator / operator-1 (#13)" in texts["06-archived-history-picker"]
assert "A partial waiting" in texts["07-restored-saved-journal"] and "Interrupted · accepted operations" in texts["07-restored-saved-journal"]
assert read("clear-running-boundary")["operator_turns"][0]["state"]=="Running"
for name in ["selected-stop","archived","restored-final"]:assert read(name+"-boundary")["operator_turns"][0]["state"]=="Interrupted"
for name in ["selected-stop","archived"]:
 before=read(name+"-boundary");after=read("restored-final-boundary")
 for table in ["operator_conversations","operator_turns","operator_messages"]:assert before[table]==after[table]
assert read("provider-finished")=={"seeded":True,"a_requests":2,"b_requests":0,"quick_requests":0,"activity_requests":0}
for name in ["gui","holder"]:assert read(name+"-exit")["original_wait_exit"]==0
cleanup=read("cleanup")
for field in ["pids","pgids"]:assert all(v=="absent" for v in cleanup[field].values())
assert not cleanup["demo_exists"] and not cleanup["socket_exists"] and cleanup["root_removed"] and cleanup["hashes_unchanged"]
for name,digest in state["source_sha256"].items():assert hashlib.sha256((repo/name).read_bytes()).hexdigest()==digest
result={"verified":True,"captures":len(texts),"recorded_pids":len(cleanup["pids"]),"recorded_groups":len(cleanup["pgids"]),"protocol":70,"schema":27}
(p/"verification.json").write_text(json.dumps(result,indent=2)+"\n");print(json.dumps(result))
