"""Zero-inference integration check. Run with NEOMIND_TEST_BIN set explicitly."""
import os
import unittest
import requests
from lib.server import TestServer

@unittest.skipUnless(os.environ.get('NEOMIND_TEST_BIN'), 'set NEOMIND_TEST_BIN to the candidate binary')
class VisionRegistrationTest(unittest.TestCase):
    def test_builtin_survives_late_backend_registration_and_removal(self):
        srv = TestServer()
        try:
            srv.spawn()
            def names():
                response = srv.get('/agents/tools')
                response.raise_for_status()
                return {tool['name'] for tool in response.json()['data']['tools']}
            self.assertIn('vision', names())
            response = srv.post('/llm-backends', {
                'name':'late-vision-test', 'backend_type':'llamacpp',
                'model':'minicpm-v-4.7-1b', 'endpoint':'http://127.0.0.1:9',
                'capabilities':{'supports_multimodal':True, 'multimodal_user_override':True},
            })
            response.raise_for_status()
            backend_id = response.json()['data']['id']
            self.assertIn('vision', names())
            response = requests.delete(srv.api_base + '/llm-backends/' + backend_id,
                headers=srv._headers(), timeout=30)
            response.raise_for_status()
            self.assertIn('vision', names())
        finally:
            srv.shutdown()

if __name__ == '__main__':
    unittest.main()
