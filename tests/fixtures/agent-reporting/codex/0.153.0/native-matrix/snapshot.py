from pathlib import Path
import json,sys,os
p=Path(__file__).parent;b=json.loads((p/'binding.json').read_text());f=Path(b['path']);assert f.is_relative_to(p/'provider');rows=[json.loads(s) for s in f.read_text().splitlines()];assert rows[0]['payload']['id']==b['thread_id']
keep=[];shapes=[]
for x in rows:
 q=x.get('payload',{});out=None
 if x['type']=='session_meta':out={k:q[k] for k in ['id','session_id','cli_version','source','history_mode','context_window'] if k in q}
 elif x['type']=='token_usage_record':out={k:q[k] for k in ['thread_id','turn_id','session_id','root_turn_id','response_id','usage','turn_token_usage','thread_token_usage'] if k in q}
 elif x['type']=='event_msg' and q.get('type')=='token_count':out={'type':'token_count','info':q.get('info')}
 elif x['type']=='event_msg':
  shapes.append({'type':q.get('type'),'keys':list(q)})
  if q.get('type') in ['task_started','task_complete','turn_aborted','error','warning']:
   out={k:q[k] for k in ['type','turn_id','model_context_window','started_at','completed_at','duration_ms','reason','codex_error_info'] if k in q}
 if out is not None:keep.append({k:x[k] for k in ['timestamp','ordinal','type'] if k in x}|{'payload':out})
stat=f.stat();result={'thread_id':b['thread_id'],'native_pid':b['native_pid'],'file_identity':{'device':stat.st_dev,'inode':stat.st_ino,'bytes':stat.st_size},'records':keep,'event_shapes':shapes}
(p/'evidence'/('snapshot-'+sys.argv[1]+'.json')).write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'snapshot':sys.argv[1],'file_identity':result['file_identity'],'rows':[x for x in keep if x['type']=='token_usage_record']}))
