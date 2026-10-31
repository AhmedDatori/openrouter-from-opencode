#!/usr/bin/env python3
"""Credential onboarding/rotation/restart checks; fake upstream and fake keys only."""
import http.server
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

from integration import ROOT, SEEN, UPSTREAM_KEY, Upstream, free_port

PASSWORD = 'credential-tests-admin-password'
ROTATED_KEY = 'rotated-upstream-test-only-key'


class Credentials(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.up = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
        threading.Thread(target=self.up.serve_forever, daemon=True).start()
        self.addCleanup(self.up.server_close)
        self.addCleanup(self.up.shutdown)
        self.port = free_port()
        self.base = f'http://127.0.0.1:{self.port}'
        self.binary = os.environ.get('GO_ROUTER_BIN', str(ROOT / 'target/debug/go-router'))
        self.env = {**os.environ, 'OPENCODE_GO_API_KEY': '', 'ADMIN_PASSWORD': PASSWORD,
                    'BIND': f'127.0.0.1:{self.port}', 'DATABASE_PATH': self.tmp.name + '/gateway.db',
                    'MODEL_CATALOG_PATH': str(ROOT / 'config/models.json'),
                    'GO_BASE_URL': f'http://127.0.0.1:{self.up.server_port}',
                    'PUBLIC_URL': self.base, 'COOKIE_SECURE': 'false'}
        self.env.pop('OPENCODE_GO_API_KEY', None)
        self.log = open(self.tmp.name + '/process.log', 'ab')
        self.addCleanup(self.log.close)
        self.proc = None
        self.addCleanup(self.stop)
        self.start()

    def stop(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            self.proc.wait(timeout=10)

    def start(self):
        self.proc = subprocess.Popen([self.binary], cwd=self.tmp.name, env=self.env,
                                     stdout=self.log, stderr=self.log)
        for _ in range(150):
            try:
                if self.call('GET', '/readyz')[0] == 200:
                    break
            except OSError:
                pass
            if self.proc.poll() is not None:
                self.fail(pathlib.Path(self.log.name).read_text())
            time.sleep(.03)
        else:
            self.fail('Gateway did not become ready')
        s, headers, result = self.call('POST', '/admin/api/login', {'password': PASSWORD})
        self.assertEqual(s, 200)
        self.cookie = headers['Set-Cookie'].split(';')[0]
        self.csrf = result['csrf_token']

    def call(self, method, path, body=None, admin=False, key=None, extra=None):
        headers = {'Content-Type': 'application/json'}
        if admin:
            headers.update({'Cookie': self.cookie, 'X-CSRF-Token': self.csrf})
        if key:
            headers['Authorization'] = 'Bearer ' + key
        headers.update(extra or {})
        raw = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(self.base + path, data=raw, headers=headers, method=method)
        try:
            response = urllib.request.urlopen(request, timeout=8)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, response.headers, json.loads(response.read())

    def test_dashboard_key_lifecycle_and_persistence(self):
        s, _, state = self.call('GET', '/admin/api/settings', admin=True)
        self.assertEqual(s, 200)
        self.assertFalse(state['upstream_configured'])
        s, _, result = self.call('POST', '/admin/api/keys', {'name': 'credential test'}, admin=True)
        self.assertEqual(s, 200)
        client_key = result['key']
        body = {'model': 'deepseek-v4.1-flash', 'messages': [{'role': 'user', 'content': 'Hello'}]}
        before = len(SEEN)
        self.assertEqual(self.call('POST', '/v1/chat/completions', body, key=client_key)[0], 503)
        self.assertEqual(before, len(SEEN))
        self.assertEqual(self.call('POST', '/admin/api/upstream-key', {'api_key': UPSTREAM_KEY})[0], 401)
        self.assertEqual(self.call('POST', '/admin/api/upstream-key', {'api_key': UPSTREAM_KEY},
                                   admin=True, extra={'X-CSRF-Token': 'bad'})[0], 403)
        self.assertEqual(self.call('POST', '/admin/api/upstream-key', {'api_key': UPSTREAM_KEY},
                                   admin=True, extra={'Origin': 'https://evil.example'})[0], 403)
        for invalid in ('', 'short', 'key-with-newline-\n-invalid', 'key with spaces invalid'):
            self.assertEqual(self.call('POST', '/admin/api/upstream-key', {'api_key': invalid}, admin=True)[0], 400)
        s, _, result = self.call('POST', '/admin/api/upstream-key', {'api_key': UPSTREAM_KEY}, admin=True)
        self.assertEqual(s, 200)
        self.assertNotIn(UPSTREAM_KEY, json.dumps(result))
        self.assertEqual(self.call('POST', '/v1/chat/completions', body, key=client_key)[0], 200)
        self.assertEqual(SEEN[-1][1]['authorization'], 'Bearer ' + UPSTREAM_KEY)
        s, _, error = self.call('POST', '/v1/chat/completions', {**body, 'test_mode': 'secret_error'}, key=client_key)
        self.assertEqual(s, 400)
        self.assertNotIn(UPSTREAM_KEY, json.dumps(error))
        self.assertEqual(self.call('POST', '/admin/api/upstream-key', {'api_key': ROTATED_KEY}, admin=True)[0], 200)
        self.assertEqual(self.call('POST', '/v1/chat/completions', body, key=client_key)[0], 200)
        self.assertEqual(SEEN[-1][1]['authorization'], 'Bearer ' + ROTATED_KEY)
        self.stop()
        self.env['OPENCODE_GO_API_KEY'] = 'legacy-key-must-not-override-dashboard'
        self.start()
        self.assertEqual(self.call('POST', '/v1/chat/completions', body, key=client_key)[0], 200)
        self.assertEqual(SEEN[-1][1]['authorization'], 'Bearer ' + ROTATED_KEY)
        for path in ('/admin/api/settings', '/admin/api/events', '/admin/api/logs?limit=200'):
            s, _, result = self.call('GET', path, admin=True)
            self.assertEqual(s, 200)
            self.assertNotIn(UPSTREAM_KEY, json.dumps(result))
            self.assertNotIn(ROTATED_KEY, json.dumps(result))
        self.assertEqual(self.call('DELETE', '/admin/api/upstream-key', admin=True)[0], 200)
        self.stop()
        self.start()
        s, _, state = self.call('GET', '/admin/api/settings', admin=True)
        self.assertFalse(state['upstream_configured'])
        before = len(SEEN)
        self.assertEqual(self.call('POST', '/v1/chat/completions', body, key=client_key)[0], 503)
        self.assertEqual(before, len(SEEN))
        if os.name != 'nt':
            self.assertEqual(pathlib.Path(self.tmp.name + '/gateway.db').stat().st_mode & 0o777, 0o600)

    def test_bad_dotenv_does_not_echo_credential(self):
        self.stop()
        secret = 'not-a-real-key-do-not-echo-on-parse-error'
        pathlib.Path(self.tmp.name, '.env').write_text('OPENCODE_GO_API_KEY="' + secret + '\n')
        result = subprocess.run([self.binary, '--check'], cwd=self.tmp.name, env=self.env,
                                capture_output=True, text=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Could not load .env', result.stderr)
        self.assertNotIn(secret, result.stderr + result.stdout)

    def test_setup_helper_requires_no_key_or_terminal_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / 'scripts').mkdir()
            shutil.copy(ROOT / 'scripts/setup.py', root / 'scripts/setup.py')
            shutil.copy(ROOT / '.env.example', root / '.env.example')
            result = subprocess.run(['python3', str(root / 'scripts/setup.py')], input='',
                                    capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('OPENCODE_GO_API_KEY=""', (root / '.env').read_text())
            env = {**os.environ, 'MODEL_CATALOG_PATH': str(ROOT / 'config/models.json')}
            env.pop('ADMIN_PASSWORD', None)
            env.pop('OPENCODE_GO_API_KEY', None)
            checked = subprocess.run([self.binary, '--check'], cwd=root, env=env,
                                     capture_output=True, text=True, timeout=5)
            self.assertEqual(checked.returncode, 0, checked.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
