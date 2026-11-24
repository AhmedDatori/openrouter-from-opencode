#!/usr/bin/env python3
"""Exercise an extracted release through real IIS/ANCM, with a local fake provider.

Run only on the disposable Windows CI runner after tests/iis-server.ps1.
The self-signed localhost certificate is deliberately accepted in this test only.
"""
import http.server
import json
import os
import pathlib
import re
import ssl
import subprocess
import threading
import time
import unittest
import urllib.error
import urllib.request

from integration import SEEN, UPSTREAM_KEY, Upstream

ROOT = pathlib.Path(os.environ['IIS_PACKAGE_ROOT'])
BASE = 'https://localhost:8443'
POOL = 'GoRouter'
TLS = ssl._create_unverified_context()


class IisPackage(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.upstream = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Upstream)
        threading.Thread(target=cls.upstream.serve_forever, daemon=True).start()
        config = (ROOT / '.env').read_text(encoding='utf-8')
        cls.password = re.search(r'^ADMIN_PASSWORD=(.+)$', config, re.M)[1]
        config = re.sub(r'^GO_BASE_URL=.*$', f'GO_BASE_URL=http://127.0.0.1:{cls.upstream.server_port}', config, flags=re.M)
        (ROOT / '.env').write_text(config, encoding='utf-8')
        cls.wait_ready()
        cls.login()

    @classmethod
    def tearDownClass(cls):
        cls.upstream.shutdown()
        cls.upstream.server_close()

    @classmethod
    def request(cls, method, path, body=None, admin=False, key=None, headers=None):
        head = {'Content-Type': 'application/json', 'Origin': BASE}
        if admin:
            head.update({'Cookie': cls.cookie, 'X-CSRF-Token': cls.csrf})
        if key:
            head['Authorization'] = 'Bearer ' + key
        head.update(headers or {})
        raw = json.dumps(body).encode() if body is not None else None
        req = urllib.request.Request(BASE + path, data=raw, method=method, headers=head)
        try:
            return urllib.request.urlopen(req, context=TLS, timeout=15)
        except urllib.error.HTTPError as error:
            return error

    @classmethod
    def call(cls, *args, **kwargs):
        with cls.request(*args, **kwargs) as result:
            raw = result.read()
            try:
                value = json.loads(raw)
            except (ValueError, UnicodeDecodeError):
                value = raw
            return result.status, result.headers, value

    @classmethod
    def wait_ready(cls):
        for _ in range(45):
            try:
                if cls.call('GET', '/healthz')[0] == 200:
                    return
            except OSError:
                pass
            time.sleep(1)
        raise RuntimeError('IIS did not start the packaged gateway; see the diagnostic logs')

    @classmethod
    def login(cls):
        status, headers, body = cls.call('POST', '/admin/api/login', {'password': cls.password})
        assert status == 200, (status, body)
        assert '; Secure' in headers['Set-Cookie']
        cls.cookie = headers['Set-Cookie'].split(';')[0]
        cls.csrf = body['csrf_token']

    def chat(self, **extra):
        return self.call('POST', '/api/v1/chat/completions', {
            'model': 'deepseek/deepseek-v4.1-flash',
            'messages': [{'role': 'user', 'content': 'Hello'}], **extra,
        }, key=self.key)

    def test_01_startup_dashboard_and_catalog(self):
        for path in ['/healthz', '/readyz', '/admin', '/admin/app.js', '/admin/style.css', '/api/v1/models']:
            self.assertEqual(self.call('GET', path)[0], 200, path)
        version = (ROOT / 'PACKAGE-INFO.txt').read_text().splitlines()[0].removeprefix('Go Router ')
        self.assertEqual(self.call('GET', '/healthz')[2]['version'], version)
        self.assertEqual(len(self.call('GET', '/api/v1/models')[2]['data']), 30)

    def test_02_credentials_and_database_are_not_served(self):
        for path in ['/.env', '/data/go-router.db', '/README.txt', '/go-router.exe', '/config/models.json', '/web.config']:
            status, _, value = self.call('GET', path)
            self.assertIn(status, [403, 404], path)
            self.assertNotIn(self.password, str(value))

    def test_03_authentication_and_origin(self):
        self.assertEqual(self.call('GET', '/admin/api/settings')[0], 401)
        self.assertEqual(self.call('POST', '/admin/api/login', {'password': self.password},
                                   headers={'Origin': 'https://other.invalid'})[0], 403)
        self.assertEqual(self.call('POST', '/admin/api/keys', {'name': 'no csrf'}, admin=True,
                                   headers={'X-CSRF-Token': 'wrong'})[0], 403)

    def test_04_create_key_without_upstream(self):
        status, _, settings = self.call('GET', '/admin/api/settings', admin=True)
        self.assertEqual(status, 200)
        self.assertFalse(settings['upstream_configured'])
        status, _, result = self.call('POST', '/admin/api/keys', {'name': 'IIS test'}, admin=True)
        self.assertEqual(status, 200, result)
        type(self).key = result['key']
        type(self).key_id = result['data']['id']
        self.assertEqual(self.chat()[0], 503)

    def test_05_save_upstream_and_complete_chat(self):
        status, _, _ = self.call('POST', '/admin/api/upstream-key', {'api_key': UPSTREAM_KEY}, admin=True)
        self.assertEqual(status, 200)
        status, _, result = self.chat()
        self.assertEqual(status, 200, result)
        self.assertEqual(result['choices'][0]['message']['content'], 'مرحبا')
        self.assertEqual(SEEN[-1][1]['authorization'], 'Bearer ' + UPSTREAM_KEY)

    def test_06_stream_arrives_before_completion(self):
        start = time.monotonic()
        with self.request('POST', '/api/v1/chat/completions', {
            'model': 'deepseek/deepseek-v4.1-flash', 'stream': True, 'test_mode': 'slow',
            'messages': [{'role': 'user', 'content': 'Hello'}],
        }, key=self.key, headers={'Accept-Encoding': 'gzip'}) as response:
            self.assertEqual(response.status, 200)
            self.assertIn('text/event-stream', response.headers['Content-Type'])
            self.assertIsNone(response.headers.get('Content-Encoding'))
            # The gateway can send SSE comments before the first answer event.
            while True:
                first = response.readline()
                if first.startswith(b'data: ') or not first:
                    break
            first_at = time.monotonic() - start
            rest = response.read()
            total = time.monotonic() - start
        self.assertTrue(first.startswith(b'data: '), first)
        self.assertIn(b'[DONE]', rest)
        self.assertLess(first_at, total - .25, f'IIS buffered SSE: first={first_at:.3f}s total={total:.3f}s')
        print(f'IIS streaming verified: first event {first_at:.3f}s, completion {total:.3f}s')

    def test_07_native_apis_and_multimodal_passthrough(self):
        for endpoint, body in [('/responses', {'input': 'Hello'}),
                               ('/messages', {'max_tokens': 50, 'messages': [{'role': 'user', 'content': 'Hello'}]})]:
            for stream in [False, True]:
                status, _, value = self.call('POST', '/api/v1' + endpoint, {
                    'model': 'deepseek/deepseek-v4.1-flash', 'stream': stream, **body,
                }, key=self.key)
                self.assertEqual(status, 200, value)
        parts = [{'type': 'image_url', 'image_url': {'url': 'data:image/png;base64,dGVzdA=='}},
                 {'type': 'text', 'text': 'What is this?'}]
        self.assertEqual(self.chat(messages=[{'role': 'user', 'content': parts}])[0], 200)
        self.assertEqual(SEEN[-1][2]['messages'][0]['content'], parts)

    def test_08_patch_and_json_errors(self):
        status, _, _ = self.call('PATCH', '/admin/api/keys/' + self.key_id, {'name': 'Updated in IIS'}, admin=True)
        self.assertEqual(status, 200)
        status, _, body = self.call('POST', '/api/v1/chat/completions', {}, key='wrong')
        self.assertEqual(status, 401)
        self.assertEqual(body['error']['code'], 401)

    def test_09_pool_stop_start_and_persistence(self):
        subprocess.run(['powershell.exe', '-NoProfile', '-Command',
                        "Import-Module WebAdministration; Stop-WebAppPool GoRouter; Start-Sleep -Seconds 3; Start-WebAppPool GoRouter"], check=True)
        self.wait_ready()
        self.login()
        settings = self.call('GET', '/admin/api/settings', admin=True)[2]
        self.assertTrue(settings['upstream_configured'])
        self.assertEqual(settings['upstream_source'], 'dashboard')
        self.assertEqual(self.chat()[0], 200)
        self.assertTrue((ROOT / 'data/go-router.db').exists())
        self.assertTrue(list((ROOT / 'data/logs').glob('stdout*.log')))


if __name__ == '__main__':
    unittest.main(verbosity=2)
