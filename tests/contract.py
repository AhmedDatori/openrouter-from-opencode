#!/usr/bin/env python3
"""Validate mock-backed gateway responses against OpenRouter's published OpenAPI.

No real Go calls or credentials. Requires requirements-dev.txt. Downloads only the
public specification unless --spec supplies a cached copy. Unknown provider names
are allowed where the official schema explicitly sets x-speakeasy-unknown-values.
"""
import argparse,json,pathlib,sys,urllib.request
from jsonschema import Draft202012Validator,validators
from referencing import Registry,Resource
from referencing.jsonschema import DRAFT202012
sys.path.insert(0,str(pathlib.Path(__file__).parent))
from integration import Gateway
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--spec',type=pathlib.Path);args=p.parse_args()
if args.spec:spec=json.loads(args.spec.read_text())
else:
 with urllib.request.urlopen('https://openrouter.ai/openapi.json',timeout=30) as response:spec=json.load(response)
original=Draft202012Validator.VALIDATORS['enum']
def compatible_enum(validator,values,instance,schema):
 if schema.get('x-speakeasy-unknown-values')!='allow':yield from original(validator,values,instance,schema)
Validator=validators.extend(Draft202012Validator,{'enum':compatible_enum})
uri='https://openrouter.ai/openapi.json'
registry=Registry().with_resource(uri,Resource.from_contents(spec,default_specification=DRAFT202012))
Gateway.setUpClass()
try:
 status,_,chat=Gateway.call('POST','/api/v1/chat/completions',{'model':'deepseek-v4.1-flash','messages':[{'role':'user','content':'x'}]},key=Gateway.key)
 assert status==200,(status,chat)
 cases=[('/models','/models',None),('/models/user','/models/user',None),('/providers','/providers',None),('/model/{author}/{slug}','/model/deepseek/deepseek-v4.1-flash',None),('/models/{author}/{slug}/endpoints','/models/deepseek/deepseek-v4.1-flash/endpoints',None),('/key','/key',None),('/generation','/generation?id='+chat['id'],None),('/chat/completions',None,chat)]
 failures=[]
 for route,path,value in cases:
  if value is None:
   status,_,value=Gateway.call('GET','/api/v1'+path,key=Gateway.key);assert status==200,(path,status,value)
  verb='post' if route=='/chat/completions' else 'get'
  pointer='/paths/'+route.replace('~','~0').replace('/','~1')+'/'+verb+'/responses/200/content/application~1json/schema'
  validation=Validator({'$ref':uri+'#'+pointer},registry=registry)
  errors=list(validation.iter_errors(value))
  if errors:
   failures.extend(f'{route} {list(e.absolute_path)}: {e.message}' for e in errors)
  else:print('PASS',route)
 if failures:raise AssertionError('\n'.join(failures))
 print(f'{len(cases)} response contracts passed against the published OpenRouter OpenAPI; forward-compatible enum annotation respected.')
finally:Gateway.tearDownClass()
