#!/usr/bin/env python3
"""Small local-overhead measurement against a mock upstream, not a model benchmark.

Uses synthetic traffic and fake credentials. Optional Docker limits prove only that
this small workload fits those limits, not that every multimodal workload will.
"""
import argparse,json,os,pathlib,statistics,subprocess,sys,tempfile,time,urllib.request
sys.path.insert(0,str(pathlib.Path(__file__).parent));from integration import Gateway,ROOT,UPSTREAM_KEY
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',default=str(ROOT/'target/release/go-router'));p.add_argument('--docker-image');p.add_argument('--requests',type=int,default=100);args=p.parse_args()
container='go-router-benchmark-'+str(os.getpid());temp=tempfile.TemporaryDirectory()
if args.docker_image:
 wrapper=pathlib.Path(temp.name)/'run-container';wrapper.write_text('''#!/usr/bin/env python3
import os
args=['docker','run','--rm','--name',%r,'--network','host','--memory','128m','--cpus','0.5','--read-only','--tmpfs','/app/data:rw,uid=10001,gid=10001,size=32m','--tmpfs','/tmp:rw,size=8m']
for name in ['BIND','GO_BASE_URL','OPENCODE_GO_API_KEY','ADMIN_PASSWORD','MAX_CONCURRENT_REQUESTS','PUBLIC_URL','COOKIE_SECURE']:
 args+=['-e',name]
args+=['-e','DATABASE_PATH=/app/data/go-router.db','-e','MODEL_CATALOG_PATH=/app/config/models.json',%r]
os.execvp('docker',args)
'''%(container,args.docker_image));wrapper.chmod(0o700);os.environ['GO_ROUTER_BIN']=str(wrapper)
else:os.environ['GO_ROUTER_BIN']=args.binary
Gateway.setUpClass()
try:
 Gateway.key=Gateway.new_key('Performance measurement',rpm=0)['key']
 body={'model':'deepseek-v4.1-flash','messages':[{'role':'user','content':'Synthetic benchmark'}]}
 def gateway():
  start=time.perf_counter();s,_,v=Gateway.call('POST','/api/v1/chat/completions',body,key=Gateway.key);assert s==200,(s,v);return (time.perf_counter()-start)*1000
 def direct():
  request=urllib.request.Request(f'http://127.0.0.1:{Gateway.up.server_port}/openai/v1/chat/completions',data=json.dumps(body).encode(),headers={'Content-Type':'application/json','Authorization':'Bearer '+UPSTREAM_KEY})
  start=time.perf_counter()
  with urllib.request.urlopen(request) as r:r.read()
  return (time.perf_counter()-start)*1000
 for _ in range(10):gateway()
 baseline=[direct() for _ in range(args.requests)];routed=[gateway() for _ in range(args.requests)]
 pid=Gateway.proc.pid
 if args.docker_image:pid=int(subprocess.check_output(['docker','inspect','-f','{{.State.Pid}}',container],text=True))
 status={}
 for line in pathlib.Path(f'/proc/{pid}/status').read_text().splitlines():
  if ':' in line:
   k,v=line.split(':',1)
   if k in ['VmRSS','VmHWM','Threads']:status[k]=v.strip()
 def summarize(values):return {'median_ms':round(statistics.median(values),3),'p95_ms':round(sorted(values)[int(len(values)*.95)-1],3),'mean_ms':round(statistics.mean(values),3)}
 result={'date_utc':'2026-10-29','workload':f'{args.requests} sequential fresh-HTTP mock chat requests per path after 10 gateway warm-ups; full payload logging enabled; no real model','requests_per_path':args.requests,'direct_mock':summarize(baseline),'gateway_and_mock':summarize(routed),'process':status,'docker_limits':{'memory_mib':128,'cpus':.5} if args.docker_image else None,'binary_bytes':pathlib.Path(args.binary).stat().st_size if pathlib.Path(args.binary).exists() else None}
 if args.docker_image:result['image_bytes']=json.loads(subprocess.check_output(['docker','image','inspect',args.docker_image],text=True))[0]['Size']
 print(json.dumps(result,indent=2))
finally:
 Gateway.tearDownClass()
 if args.docker_image:subprocess.run(['docker','rm','-f',container],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 temp.cleanup()
