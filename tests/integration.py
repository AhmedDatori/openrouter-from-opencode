#!/usr/bin/env python3
"""Black-box compatibility tests; standard library only, no real API credentials."""
import contextlib,http.client,http.server,json,os,pathlib,secrets,socket,subprocess,tempfile,threading,time,unittest,urllib.request,urllib.error
ROOT=pathlib.Path(__file__).resolve().parents[1]
UPSTREAM_KEY='upstream-test-secret-not-a-real-key'
PASSWORD='integration-only-password-123456'
SEEN=[]
USAGE={'prompt_tokens':12,'completion_tokens':8,'total_tokens':20,'cost':0,'opencode':{'billing_source':'go','allowance':.0015,'list_cost':.002,'tokens':{'input':12,'output':8,'reasoning':3}}}
class Upstream(http.server.BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers['Content-Length'])));SEEN.append((self.path,dict(self.headers),body))
  model=body['model'];mode=body.get('test_mode','')
  if mode=='fallback' and model=='deepseek-v4.1-flash':return self.respond(429,{'error':{'message':'model capacity exhausted'}},{'Retry-After':'3'})
  if mode=='pre_error':return self.respond(403,{'error':{'message':'privacy policy blocks model'}})
  if mode=='secret_error':return self.respond(400,{'error':{'message':UPSTREAM_KEY+' is invalid'}})
  if mode=='bad_json':return self.respond(200,b'not-json')
  if mode=='bad_shape':return self.respond(200,{'hello':'invalid upstream'})
  if mode=='hold':time.sleep(.7)
  if self.path.endswith('/responses'):
   result={'error':None,'id':'resp_'+secrets.token_hex(6),'object':'response','status':'completed','model':model,'output':[{'id':'msg1','type':'message','status':'completed','role':'assistant','content':[{'type':'output_text','text':'Hello','annotations':[]}]}],'usage':{'input_tokens':12,'output_tokens':8,'total_tokens':20,'cost':0,'opencode':USAGE['opencode']}}
   if body.get('stream'):return self.sse([('response.created',{'type':'response.created','response':{**result,'status':'in_progress','output':[]}}),('response.output_text.delta',{'type':'response.output_text.delta','delta':'Hello','output_index':0,'content_index':0}),('response.completed',{'type':'response.completed','response':result})])
   return self.respond(200,result)
  if self.path.endswith('/messages'):
   result={'id':'msg_'+secrets.token_hex(6),'type':'message','role':'assistant','model':model,'content':[{'type':'text','text':'Hello'}],'stop_reason':'end_turn','stop_sequence':None,'usage':{'input_tokens':12,'output_tokens':8,'cost':0,'opencode':USAGE['opencode']}}
   if body.get('stream'):return self.sse([('message_start',{'type':'message_start','message':{**result,'content':[],'usage':{'input_tokens':12,'output_tokens':0}}}),('content_block_delta',{'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'Hello'}}),('message_delta',{'type':'message_delta','delta':{'stop_reason':'end_turn'},'usage':result['usage']}),('message_stop',{'type':'message_stop'})])
   return self.respond(200,result)
  result={'id':'chat_'+secrets.token_hex(6),'object':'chat.completion','created':123,'model':model,'choices':[{'index':0,'message':{'role':'assistant','content':'مرحبا','reasoning_content':'private thought'},'finish_reason':'stop'}],'usage':USAGE}
  if body.get('tools'):
   result['choices'][0]['message']={'role':'assistant','content':None,'tool_calls':[{'id':'call_1','type':'function','function':{'name':'lookup','arguments':'{"id":1}'}}]};result['choices'][0]['finish_reason']='tool_calls'
  if body.get('stream'):
   if mode=='stall':
    self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Connection','close');self.end_headers();self.wfile.flush();time.sleep(2);self.close_connection=True;return
   chunks=[(None,{'id':result['id'],'choices':[{'index':0,'delta':{'role':'assistant','content':'مر'},'finish_reason':None}]}),(None,{'id':result['id'],'choices':[{'index':0,'delta':{'content':'حبا'},'finish_reason':'stop'}]})]
   if mode=='mid_error':chunks.append((None,{'error':{'message':'stream failed','code':502}}))
   elif mode=='truncate':return self.sse(chunks,done=False)
   else:chunks.append((None,{'id':result['id'],'choices':[],'usage':USAGE}))
   return self.sse(chunks,done=True,slow=mode=='slow')
  return self.respond(200,result)
 def respond(self,status,value,headers=None):
  raw=value if isinstance(value,bytes) else json.dumps(value,ensure_ascii=False).encode();self.send_response(status);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(raw)))
  for k,v in (headers or {}).items():self.send_header(k,v)
  self.end_headers();self.wfile.write(raw)
 def sse(self,frames,done=False,slow=False):
  self.send_response(200);self.send_header('Content-Type','text/event-stream');self.send_header('Connection','close');self.end_headers()
  try:
   for i,(event,data) in enumerate(frames):
    if slow and i:time.sleep(.35)
    raw=((('event: '+event+'\r\n') if event else '')+'data: '+json.dumps(data,ensure_ascii=False)+'\r\n\r\n').encode()
    # Deliberately fragment across UTF-8 and SSE boundaries.
    for p in range(0,len(raw),7):self.wfile.write(raw[p:p+7]);self.wfile.flush()
   if done:self.wfile.write(b'data: [DONE]\r\n\r\n');self.wfile.flush()
  except (BrokenPipeError,ConnectionResetError):pass
  self.close_connection=True

def free_port():
 with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
class Gateway(unittest.TestCase):
 @classmethod
 def setUpClass(cls):
  cls.tmp=tempfile.TemporaryDirectory();cls.up=http.server.ThreadingHTTPServer(('127.0.0.1',0),Upstream);threading.Thread(target=cls.up.serve_forever,daemon=True).start()
  cls.port=free_port();cls.base=f'http://127.0.0.1:{cls.port}';env={**os.environ,'BIND':f'127.0.0.1:{cls.port}','DATABASE_PATH':cls.tmp.name+'/test.db','GO_BASE_URL':f'http://127.0.0.1:{cls.up.server_port}','OPENCODE_GO_API_KEY':UPSTREAM_KEY,'ADMIN_PASSWORD':PASSWORD,'MODEL_CATALOG_PATH':str(ROOT/'config/models.json'),'MAX_CONCURRENT_REQUESTS':'2','MAX_BODY_BYTES':'1048576','PUBLIC_URL':cls.base,'COOKIE_SECURE':'false','STREAM_IDLE_TIMEOUT_SECONDS':'1'}
  cls.log=open(cls.tmp.name+'/server.log','wb');binary=os.environ.get('GO_ROUTER_BIN',str(ROOT/'target/debug/go-router'));cls.proc=subprocess.Popen([binary],cwd=cls.tmp.name,env=env,stdout=cls.log,stderr=cls.log)
  for _ in range(150):
   try:
    if urllib.request.urlopen(cls.base+'/healthz',timeout=.2).status==200:break
   except Exception:
    if cls.proc.poll() is not None:raise RuntimeError(pathlib.Path(cls.tmp.name+'/server.log').read_text())
    time.sleep(.05)
  else:raise RuntimeError('Server failed to start')
  status,h,data=cls.call('POST','/admin/api/login',{'password':PASSWORD});assert status==200,(status,data)
  cls.cookie=h['Set-Cookie'].split(';')[0];cls.csrf=data['csrf_token']
  cls.key=cls.new_key('integration')['key']
 @classmethod
 def tearDownClass(cls):
  cls.proc.terminate();cls.proc.wait(timeout=10);cls.up.shutdown();cls.up.server_close();cls.log.close();cls.tmp.cleanup()
 @classmethod
 def call(cls,method,path,body=None,key=None,admin=False,headers=None):
  hh={'Content-Type':'application/json'}
  if key:hh['Authorization']='Bearer '+key
  if admin:hh.update({'Cookie':cls.cookie,'X-CSRF-Token':cls.csrf})
  hh.update(headers or {});raw=body if isinstance(body,bytes) else json.dumps(body).encode() if body is not None else None
  try:
   r=urllib.request.urlopen(urllib.request.Request(cls.base+path,data=raw,headers=hh,method=method),timeout=8);data=r.read();status=r.status;rh=r.headers
  except urllib.error.HTTPError as e:data=e.read();status=e.code;rh=e.headers;e.close()
  try:data=json.loads(data)
  except (ValueError,UnicodeDecodeError):pass
  return status,rh,data
 @classmethod
 def new_key(cls,name,**kwargs):
  status,_,data=cls.call('POST','/admin/api/keys',{'name':name,**kwargs},admin=True);assert status==200,(status,data);return data
 def chat(self,**kwargs):return self.call('POST','/api/v1/chat/completions',{'model':'deepseek/deepseek-v4.1-flash','messages':[{'role':'user','content':'Hello'}],**kwargs},key=self.key)
 def test_01_authentication(self):
  for key in [None,'invalid']:
   s,_,v=self.call('POST','/api/v1/chat/completions',{},key=key);self.assertEqual(s,401);self.assertEqual(v['error']['code'],401)
 def test_02_models_and_aliases(self):
  s,_,v=self.call('GET','/api/v1/models');self.assertEqual(s,200);self.assertEqual(len(v['data']),30);self.assertTrue(all('go_router' in m for m in v['data']))
  s,_,v=self.chat(model='opencode-go/deepseek-v4.1-flash');self.assertEqual(s,200);self.assertEqual(v['model'],'deepseek/deepseek-v4.1-flash')
 def test_03_plain_response_and_usage(self):
  s,h,v=self.chat();self.assertEqual(s,200);self.assertEqual(v['choices'][0]['message']['content'],'مرحبا');self.assertEqual(v['usage']['cost'],0);self.assertEqual(v['usage']['opencode']['allowance'],.0015);self.assertEqual(v['usage']['completion_tokens_details']['reasoning_tokens'],3);self.assertEqual(h['X-Generation-Id'],v['id'])
  path,headers,body=SEEN[-1];self.assertEqual(headers['authorization'],'Bearer '+UPSTREAM_KEY);self.assertIn('x-opencode-session',headers);self.assertNotIn(self.key,json.dumps(headers));self.assertEqual(body['model'],'deepseek-v4.1-flash')
 def test_04_streaming_utf8_and_usage_shape(self):
  s,h,raw=self.chat(stream=True);self.assertEqual(s,200);self.assertIn('text/event-stream',h['Content-Type']);frames=[json.loads(l[6:]) for l in raw.decode().splitlines() if l.startswith('data: ') and l!='data: [DONE]'];self.assertEqual(''.join(f['choices'][0]['delta'].get('content','') for f in frames),'مرحبا');self.assertTrue(frames[-1]['choices']);self.assertEqual(frames[-1]['usage']['cost'],0);self.assertTrue(raw.endswith(b'data: [DONE]\n\n'))
 def test_05_tool_calls_and_history(self):
  tool={'type':'function','function':{'name':'lookup','parameters':{'type':'object','properties':{'id':{'type':'integer'}}}}};s,_,v=self.chat(tools=[tool],tool_choice='auto');self.assertEqual(s,200);self.assertEqual(v['choices'][0]['finish_reason'],'tool_calls');self.assertEqual(SEEN[-1][2]['tools'],[tool]);messages=[{'role':'user','content':'Find record'},v['choices'][0]['message'],{'role':'tool','tool_call_id':'call_1','content':'found'}];s,_,_=self.chat(messages=messages);self.assertEqual(s,200);self.assertEqual(SEEN[-1][2]['messages'][-1]['tool_call_id'],'call_1')
 def test_06_reasoning_and_output_schema(self):
  fmt={'type':'json_schema','json_schema':{'name':'test','strict':True,'schema':{'type':'object','properties':{}}}};s,_,v=self.chat(reasoning={'effort':'low','exclude':True},response_format=fmt,max_tokens=250);self.assertEqual(s,200);self.assertNotIn('reasoning_content',v['choices'][0]['message']);self.assertEqual(SEEN[-1][2]['reasoning_effort'],'low');self.assertEqual(SEEN[-1][2]['max_completion_tokens'],250);self.assertEqual(SEEN[-1][2]['response_format'],fmt)
 def test_07_fallbacks_only_before_output(self):
  s,_,v=self.chat(models=['glm-5.3-flash'],test_mode='fallback');self.assertEqual(s,200);self.assertTrue(v['model'].endswith('glm-5.3-flash'));self.assertEqual(SEEN[-2][2]['model'],'deepseek-v4.1-flash')
 def test_08_provider_constraints(self):
  for provider in [{'only':['OtherProvider']},{'ignore':['OpenCode Go']},{'zdr':True,'only':['opencode-go']}]:
   model='beam-free' if 'zdr' in provider else 'deepseek-v4.1-flash';s,_,_=self.chat(model=model,provider=provider);self.assertEqual(s,503)
  s,_,_=self.chat(provider={'data_collection':'deny','only':['OpenCode Go']});self.assertEqual(s,200)
 def test_09_unsupported_features_are_explicit(self):
  for extra in [{'plugins':[{'id':'web'}]},{'reasoning':{'max_tokens':50}},{'modalities':['audio']}]:self.assertEqual(self.chat(**extra)[0],501)
  s,_,v=self.call('POST','/api/v1/embeddings',{},key=self.key);self.assertEqual(s,501);self.assertIn('error',v)
 def test_10_model_scope_and_revocation(self):
  made=self.new_key('restricted',allowed_models=['glm-5.3-flash']);key=made['key'];s,_,_=self.call('POST','/api/v1/chat/completions',{'model':'deepseek-v4.1-flash','messages':[{'role':'user','content':'x'}]},key=key);self.assertEqual(s,403)
  s,_,v=self.call('GET','/api/v1/models/user',key=key);self.assertEqual(len(v['data']),1)
  self.call('DELETE','/admin/api/keys/'+made['data']['id'],admin=True);self.assertEqual(self.call('GET','/api/v1/key',key=key)[0],401)
 def test_11_rate_limit(self):
  key=self.new_key('limited',rpm=1)['key'];body={'messages':[{'role':'user','content':'x'}]};self.assertEqual(self.call('POST','/v1/chat/completions',body,key=key)[0],200);s,h,v=self.call('POST','/v1/chat/completions',body,key=key);self.assertEqual(s,429);self.assertIn('Retry-After',h)
 def test_12_admin_csrf_origin_and_secret_storage(self):
  self.assertEqual(self.call('POST','/admin/api/keys',{'name':'bad'},admin=True,headers={'X-CSRF-Token':'wrong'})[0],403)
  self.assertEqual(self.call('GET','/admin/api/stats',admin=True,headers={'Origin':'https://evil.example'})[0],403)
  s,_,v=self.call('GET','/admin/api/keys',admin=True);self.assertEqual(s,200);self.assertNotIn(self.key,json.dumps(v));self.assertNotIn('hash',json.dumps(v))
  s,_,v=self.call('GET','/admin/api/settings',admin=True);self.assertEqual(s,200);self.assertNotIn(UPSTREAM_KEY,json.dumps(v))
 def test_13_generation_ownership(self):
  _,_,v=self.chat();gid=v['id'];s,_,r=self.call('GET','/api/v1/generation?id='+gid,key=self.key);self.assertEqual(s,200);self.assertEqual(r['data']['total_cost'],0)
  other=self.new_key('other')['key'];self.assertEqual(self.call('GET','/api/v1/generation?id='+gid,key=other)[0],404)
 def test_14_media_passthrough_and_capability_check(self):
  parts=[{'type':'text','text':'Read image'},{'type':'image_url','image_url':{'url':'data:image/png;base64,aGVsbG8='}}];self.assertEqual(self.chat(messages=[{'role':'user','content':parts}])[0],200);self.assertEqual(SEEN[-1][2]['messages'][0]['content'],parts)
  self.assertEqual(self.chat(model='beam-free',messages=[{'role':'user','content':parts}])[0],400)
 def test_15_native_responses(self):
  for stream in [False,True]:
   s,_,v=self.call('POST','/api/v1/responses',{'model':'gpt-6-luna','input':'Hello','stream':stream},key=self.key);self.assertEqual(s,200)
   if stream:self.assertIn(b'event: response.completed',v)
   else:self.assertEqual(v['object'],'response');self.assertTrue(v['id'].startswith('resp_'))
  self.assertEqual(self.call('POST','/api/v1/responses',{'input':'More','previous_response_id':'resp_other'},key=self.key)[0],501)
 def test_16_anthropic_messages(self):
  for stream in [False,True]:
   s,_,v=self.call('POST','/api/v1/messages',{'model':'claude-haiku-5-5','messages':[{'role':'user','content':'Hello'}],'max_tokens':100,'stream':stream},key=self.key,headers={'Anthropic-Version':'2023-06-01'});self.assertEqual(s,200)
   if stream:self.assertIn(b'event: message_stop',v)
   else:self.assertEqual(v['type'],'message');self.assertEqual(v['usage']['cost'],0)
 def test_17_legacy_completions(self):
  s,_,v=self.call('POST','/api/v1/completions',{'model':'beam-free','prompt':'Hello'},key=self.key);self.assertEqual(s,200);self.assertEqual(v['object'],'text_completion');self.assertEqual(v['choices'][0]['text'],'مرحبا')
 def test_18_pre_stream_and_mid_stream_errors(self):
  s,_,v=self.chat(stream=True,test_mode='pre_error');self.assertEqual(s,403);self.assertEqual(v['error']['code'],403)
  s,_,v=self.chat(stream=True,test_mode='mid_error');self.assertEqual(s,200);self.assertIn(b'"code":502',v);self.assertIn(b'[DONE]',v)
  s,_,v=self.chat(stream=True,test_mode='truncate');self.assertEqual(s,200);self.assertIn(b'without a terminal event',v)
 def test_19_request_errors_and_secret_redaction(self):
  self.assertEqual(self.call('POST','/api/v1/chat/completions',b'{oops',key=self.key)[0],400)
  self.assertEqual(self.chat(model='not-a-go-model')[0],404)
  s,_,v=self.chat(test_mode='secret_error');self.assertEqual(s,400);self.assertNotIn(UPSTREAM_KEY,json.dumps(v))
  self.assertEqual(self.chat(test_mode='bad_json')[0],502)
  self.assertEqual(self.chat(test_mode='bad_shape')[0],502)
 def test_20_streaming_is_not_buffered(self):
  conn=http.client.HTTPConnection('127.0.0.1',self.port,timeout=3);body=json.dumps({'model':'deepseek-v4.1-flash','messages':[{'role':'user','content':'Hi'}],'stream':True,'test_mode':'slow'});start=time.monotonic();conn.request('POST','/api/v1/chat/completions',body,{'Authorization':'Bearer '+self.key,'Content-Type':'application/json'});r=conn.getresponse()
  while True:
   line=r.readline()
   if b'"content":"' in line:break
   self.assertTrue(line)
  self.assertLess(time.monotonic()-start,.3);r.read();self.assertGreater(time.monotonic()-start,.6);conn.close()
 def test_21_cancelled_stream_is_logged(self):
  conn=http.client.HTTPConnection('127.0.0.1',self.port,timeout=3);body=json.dumps({'messages':[{'role':'user','content':'cancel'}],'stream':True,'test_mode':'slow'});conn.request('POST','/api/v1/chat/completions',body,{'Authorization':'Bearer '+self.key,'Content-Type':'application/json'});r=conn.getresponse();gid=r.getheader('X-Generation-Id')
  while True:
   line=r.readline()
   if b'"content":"' in line:break
  r.close();conn.close()
  for _ in range(30):
   s,_,v=self.call('GET','/admin/api/logs/'+gid,admin=True)
   if s==200:break
   time.sleep(.05)
  self.assertEqual(s,200);self.assertEqual(v['outcome'],'client_disconnected');self.assertIsNone(v['usage']);self.assertTrue(v['response']['stream_events'])
 def test_22_logs_stats_and_catalog_reload(self):
  s,_,v=self.call('GET','/admin/api/logs?limit=200',admin=True);self.assertEqual(s,200);self.assertGreater(v['total'],10)
  s,_,v=self.call('GET','/admin/api/stats',admin=True);self.assertEqual(s,200);self.assertGreater(v['totals']['allowance_usd'],0);self.assertEqual(v['totals']['cash_usd'],0)
  self.assertEqual(self.call('POST','/admin/api/models/reload',{},admin=True)[0],200)
 def test_23_concurrency_limit(self):
  results=[]
  def hold():results.append(self.chat(test_mode='hold')[0])
  threads=[threading.Thread(target=hold) for _ in range(2)]
  for t in threads:t.start()
  time.sleep(.15);self.assertEqual(self.chat()[0],429)
  for t in threads:t.join()
  self.assertEqual(results,[200,200])
 def test_24_static_dashboard_and_health(self):
  for path in ['/admin','/admin/app.js','/admin/style.css','/readyz']:
   s,h,_=self.call('GET',path);self.assertEqual(s,200)
   if path.startswith('/admin'):self.assertIn('Content-Security-Policy',h)
 def test_25_native_errors_and_method_errors(self):
  s,_,v=self.call('POST','/api/v1/messages',{'model':'missing','messages':[]},key=self.key);self.assertEqual(s,404);self.assertEqual(v['type'],'error');self.assertEqual(v['error']['type'],'not_found_error')
  s,_,v=self.call('POST','/api/v1/responses',{'input':'x'},key='wrong');self.assertEqual(s,401);self.assertEqual(v['error']['type'],'authentication_error')
  s,_,v=self.call('GET','/api/v1/chat/completions');self.assertEqual(s,405);self.assertEqual(v['error']['code'],405)
 def test_26_audio_pdf_and_native_passthrough(self):
  parts=[{'type':'input_audio','input_audio':{'data':'dGVzdA==','format':'wav'}}];s,_,_=self.chat(model='mimo-v2.6-flash',messages=[{'role':'user','content':parts}]);self.assertEqual(s,200);self.assertEqual(SEEN[-1][2]['messages'][0]['content'],parts)
  parts=[{'type':'file','file':{'filename':'invoice.pdf','file_data':'data:application/pdf;base64,dGVzdA=='}}];self.assertEqual(self.chat(model='gemini-4-argon',messages=[{'role':'user','content':parts}])[0],200)
 def test_27_redaction_and_key_expiry(self):
  _,_,v=self.chat(messages=[{'role':'user','content':[{'type':'image_url','image_url':{'url':'data:image/png;base64,dGVzdA=='}},{'type':'text','text':'x'}]}]);s,_,log=self.call('GET','/admin/api/logs/'+v['id'],admin=True);self.assertEqual(s,200);self.assertNotIn('data:image/png',json.dumps(log['request']))
  s,_,_=self.call('POST','/admin/api/keys',{'name':'expired','expires_at':1},admin=True);self.assertEqual(s,400)
  self.assertEqual(self.call('GET','/admin/api/events',admin=True)[0],200)
 def test_28_idle_stream_timeout(self):
  start=time.monotonic();s,_,raw=self.chat(stream=True,test_mode='stall');self.assertEqual(s,200);self.assertIn(b'idle timeout',raw);self.assertLess(time.monotonic()-start,1.7)
 def test_29_anthropic_api_key_header(self):
  s,_,v=self.call('POST','/api/v1/messages',{'model':'claude-haiku-5-5','messages':[{'role':'user','content':'Hello'}],'max_tokens':100},headers={'x-api-key':self.key});self.assertEqual(s,200);self.assertEqual(v['type'],'message')
 def test_30_generation_without_usage_has_no_fake_zero_cost(self):
  _,h,_=self.chat(stream=True,test_mode='mid_error');s,_,v=self.call('GET','/api/v1/generation?id='+h['X-Generation-Id'],key=self.key);self.assertEqual(s,409);self.assertIn('no finalized usage',v['error']['message'])
 def test_31_catalog_pagination(self):
  s,_,v=self.call('GET','/api/v1/models?limit=7');self.assertEqual(s,200);self.assertEqual(len(v['data']),7);self.assertEqual(v['total_count'],30);self.assertIsNotNone(v['links']['next']);s,_,next_page=self.call('GET',v['links']['next']);self.assertEqual(s,200);self.assertNotEqual(next_page['data'][0]['id'],v['data'][0]['id']);self.assertEqual(self.call('GET','/api/v1/models?limit=0')[0],400)
if __name__=='__main__':unittest.main(verbosity=2)
