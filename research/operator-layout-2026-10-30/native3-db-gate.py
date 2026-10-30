import pathlib,json,time,sqlite3,os
p=pathlib.Path.cwd()/"research/operator-layout-2026-10-30";s=json.loads((p/"native3-state.json").read_text());r=pathlib.Path(s["root"]);c=r/"control";deadline=time.monotonic()+30
while not (c/"db-go").exists():
 assert time.monotonic()<deadline,"native gate never started"
 time.sleep(.005)
db=sqlite3.connect(str(pathlib.Path(s["demo"])/"registry.sqlite3"),timeout=1);db.execute("BEGIN IMMEDIATE");held=time.monotonic();(c/"db-held").write_text("held")
try:
 while not (c/"db-release").exists() and time.monotonic()-held<3:time.sleep(.005)
finally:db.rollback();db.close()
(p/"native3-db-gate.json").write_text(json.dumps({"pid":os.getpid(),"max_hold_seconds":3,"observed_hold_seconds":time.monotonic()-held,"action":"BEGIN IMMEDIATE; ROLLBACK; no writes","rusqlite_default_busy_timeout_ms":5000},indent=2)+"\n")
print("gate original exit 0",flush=True)
