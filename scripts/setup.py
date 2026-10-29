#!/usr/bin/env python3
"""Create a private .env; Python is needed only for this optional setup helper."""
import argparse,getpass,json,os,pathlib,secrets
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--key-file',type=pathlib.Path,help='Read the Go key from a private text file');p.add_argument('--origin',default='http://localhost:8080');p.add_argument('--bind',default='127.0.0.1:8080');args=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[1];target=root/'.env'
if target.exists():raise SystemExit('.env already exists. Edit it locally; this helper never overwrites credentials.')
key=args.key_file.read_text().strip() if args.key_file else getpass.getpass('OpenCode Go API key (hidden): ').strip()
if not key:raise SystemExit('A Go API key is required.')
password=secrets.token_urlsafe(32)
# JSON string quoting is accepted by dotenv and avoids shell evaluation.
text=(root/'.env.example').read_text().replace('OPENCODE_GO_API_KEY=replace-with-your-opencode-go-key','OPENCODE_GO_API_KEY='+json.dumps(key)).replace('ADMIN_PASSWORD=replace-with-a-long-random-password','ADMIN_PASSWORD='+json.dumps(password)).replace('BIND=127.0.0.1:8080','BIND='+args.bind).replace('PUBLIC_URL=http://localhost:8080','PUBLIC_URL='+args.origin).replace('COOKIE_SECURE=false','COOKIE_SECURE='+str(args.origin.startswith('https://')).lower())
fd=os.open(target,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
with os.fdopen(fd,'w') as f:f.write(text)
data=root/'data';data.mkdir(exist_ok=True);login=data/'admin-login.txt'
fd=os.open(login,os.O_CREAT|os.O_EXCL|os.O_WRONLY,0o600)
with os.fdopen(fd,'w') as f:f.write('Go Router admin\nURL: '+args.origin+'/admin\nPassword: '+password+'\n\nAPI base URL: '+args.origin+'/api/v1\nCreate application keys in the admin dashboard. Never use the Go upstream key as a client key.\n')
print('Created .env (mode 600). Admin login details: data/admin-login.txt. No credentials printed.')
