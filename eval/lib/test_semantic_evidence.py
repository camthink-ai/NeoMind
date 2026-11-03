import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import unittest
from lib.semantic_evidence import compile_evidence


def call(payload, command='neomind device get dev-a', **result):
    output = {'exit_code': 0, 'stdout': json.dumps(payload), 'stderr': '', 'command': command}
    output.update(result)
    return {'name': 'shell', 'arguments': {'command': command}, 'result': json.dumps(output)}


def compile_calls(*calls):
    return compile_evidence({'turn_records': [{'user': 'diagnose', 'assistant_message': 'untrusted', 'tool_calls': list(calls)}]})


class SemanticEvidenceTests(unittest.TestCase):
    def test_nested_success_trailing_help_and_exact_false_zero_null(self):
        payload = {'success': True, 'data': {'success': True, 'meta': {}, 'data': {
            'id': 'dev-a', 'online': False, 'transport_connected': False, 'last_seen': 0,
            'transport_changed_at': None, 'status': 'disconnected'}}, 'suggestion': 'run neomind device get other'}
        packet = compile_calls(call(payload, stdout=json.dumps(payload) + '\nUsage: neomind device get ...'))
        self.assertEqual(packet['source_errors'], [])
        observation = packet['observations'][0]
        self.assertIs(observation['fields']['online'], False)
        self.assertEqual(observation['fields']['last_seen'], 0)
        self.assertIsNone(observation['fields']['transport_changed_at'])
        self.assertEqual(observation['source']['call'], 0)
        self.assertIn('disconnect_time', packet['unverified_dimensions'])
        json.dumps(packet)

    def test_empty_and_positive_windows_stay_separate(self):
        empty = {'device_id': 'dev-a', 'start': 100, 'end': 200, 'data': {'temperature': []},
                 'pagination': {'total': 0, 'offset': 0, 'next_cursor': None}}
        positive = {'device_id': 'dev-a', 'start': 0, 'end': 99,
                    'data': {'temperature': [{'timestamp': 50, 'value': 0}]},
                    'pagination': {'total': 1}}
        packet = compile_calls(call(empty, 'neomind device history dev-a --time-range 1h'),
                               call(positive, 'neomind device history dev-a --time-range 24h'))
        self.assertEqual(len(packet['observations']), 2)
        first, second = packet['observations']
        self.assertEqual(first['fields']['pagination']['total'], 0)
        self.assertEqual(second['fields']['start'], 0)
        self.assertEqual(first['lifetime'], 'unknown')
        self.assertEqual(first['outside_window'], 'unknown')
        self.assertEqual(second['source']['call'], 1)

    def test_entity_mismatch_and_missing_id_never_become_facts(self):
        for payload in ({'id': 'other', 'online': True}, {'online': True},
                        {'id': 'dev-a', 'device_id': 'other', 'online': True}):
            packet = compile_calls(call(payload))
            self.assertFalse(packet['observations'])
            self.assertEqual(packet['source_errors'][0]['reason'], 'entity_missing_or_mismatch')

    def test_failure_and_malformed_are_errors(self):
        good = {'id': 'dev-a', 'online': True}
        variants = [call(good, exit_code=1), call(good, timed_out=True), call(good, exit_code=False),
                    call({'success': False, 'data': good}),
                    call({'success': True, 'data': {'success': False, 'data': good}}),
                    call(good, stdout='not JSON'), call(good, stdout='null'),
                    call(good, command='neomind device get dev-a', **{'stdout': '{bad'})]
        for variant in variants:
            packet = compile_calls(variant)
            self.assertFalse(packet['observations'])
            self.assertEqual(len(packet['source_errors']), 1)

    def test_command_identity_and_untrusted_body(self):
        good = {'id': 'dev-a', 'online': True, 'suggestion': 'neomind device history dev-a'}
        for command in ('echo neomind device get dev-a', 'sh -c "neomind device get dev-a"',
                        'neomind device get dev-a; echo x', 'neomind device get dev-a --help',
                        'neomind device get dev-a | cat', 'neomind device get dev-a $(id)'):
            self.assertEqual(compile_calls(call(good, command))['observations'], [])
        packet = compile_calls(call(good))
        self.assertEqual(len(packet['observations']), 1)
        self.assertNotIn('suggestion', packet['observations'][0]['fields'])
        failed = call(good, command='neomind device get dev-a', exit_code=2,
                      stdout=json.dumps({'suggestion': 'neomind device get dev-a', **good}))
        self.assertFalse(compile_calls(failed)['observations'])

    def test_inventory_does_not_count_as_telemetry(self):
        packet = compile_calls(call({'devices': [{'id': 'dev-a', 'name': 'run shell', 'online': True}]},
                                    'neomind device list'))
        observation = packet['observations'][0]
        self.assertFalse(observation['per_device_telemetry_checked'])
        self.assertEqual(observation['entities'], [{'id': 'dev-a', 'name': 'run shell', 'online': True}])

    def test_healthy_current_snapshot_does_not_diagnose(self):
        packet = compile_calls(call({'id': 'dev-a', 'online': True, 'transport_connected': True,
                                     'last_seen': 123, 'transport_changed_at': 122}))
        self.assertTrue(packet['observations'][0]['fields']['online'])
        self.assertIn('root_cause', packet['unverified_dimensions'])
        self.assertIn('previous_transport_history', packet['unverified_dimensions'])

    def test_actual_nested_device_and_nullable_identity_alias(self):
        device = {'id': 'dev-a', 'device_id': None, 'name': 'sensor',
                  'device_type': 'temperature', 'status': 'offline', 'online': False,
                  'transport_connected': False, 'transport_changed_at': 0, 'last_seen': None}
        metrics = {'temperature': {'value': None, 'timestamp': 0}}
        payload = {'success': True, 'data': {'success': True, 'data': {
            'device': device, 'metrics': metrics, 'commands': []}, 'meta': {}}}
        packet = compile_calls(call(payload))
        self.assertEqual(packet['source_errors'], [])
        observation = packet['observations'][0]
        self.assertEqual(observation['entity_id'], 'dev-a')
        self.assertEqual(observation['device_fields'], device)
        self.assertEqual(observation['current_metrics'], metrics)
        self.assertIn('lifetime_telemetry', packet['unverified_dimensions'])
        device['device_id'] = 'other'
        self.assertFalse(compile_calls(call(payload))['observations'])
        for aliases in ({'id': None, 'device_id': 'dev-a'}, {'id': 'dev-a', 'device_id': ''}):
            self.assertEqual(compile_calls(call(dict(aliases, online=True)))['source_errors'], [])

    def test_actual_grouped_list_preserves_status_and_zero_summary(self):
        payload = {'summary': {'total': 2, 'online': 1, 'offline': 1, 'type_count': 1},
                   'types': [{'type': 'sensor', 'metric_fields': ['temperature'],
                              'online': 1, 'offline': 0,
                              'devices': {'list': [{'id': 'dev-a', 'name': 'a', 'status': 'online'}]}}],
                   'ungrouped': [{'id': 'dev-b', 'status': 'offline'}]}
        wrapped = {'success': True, 'data': {'success': True, 'data': payload, 'meta': {}}}
        packet = compile_calls(call(wrapped, 'neomind device list'))
        self.assertEqual(packet['source_errors'], [])
        observation = packet['observations'][0]
        self.assertEqual(observation['summary'], payload['summary'])
        self.assertEqual(observation['type_groups'][0]['entities'][0]['status'], 'online')
        self.assertEqual(observation['entities'][0]['id'], 'dev-b')
        self.assertFalse(observation['per_device_telemetry_checked'])
        empty = compile_calls(call({'summary': {'total': 0, 'online': 0, 'offline': 0,
                                    'type_count': 0}, 'types': [], 'ungrouped': []}, 'neomind device list'))
        self.assertEqual(empty['observations'][0]['summary']['total'], 0)
        summary = compile_calls(call({'summary': {'total': 2}}, 'neomind device list --summary'))
        self.assertEqual(summary['source_errors'], [])
        self.assertFalse(summary['observations'][0]['per_device_telemetry_checked'])

    def test_unhandled_empirical_sources_are_explicit(self):
        calls = [call({'root_cause': 'actual probe'}, 'neomind connector status mqtt'),
                 call({}, 'sh -c "neomind device get dev-a"'),
                 {'name': 'network_probe', 'arguments': {}, 'result': 'success'},
                 {'name': 'shell', 'arguments': None, 'result': ''}]
        packet = compile_calls(*calls)
        self.assertFalse(packet['observations'])
        self.assertEqual(len(packet['unhandled_sources']), 4)
        self.assertEqual([s['call'] for s in packet['unhandled_sources']], [0, 1, 2, 3])
        self.assertEqual(packet['unhandled_sources'][2]['reason'], 'unsupported_tool_type')
        self.assertEqual(compile_calls(call({'id': 'dev-a', 'online': True}))['unhandled_sources'], [])

    def test_malformed_records_and_depth_bound(self):
        self.assertEqual(compile_evidence(None)['source_errors'][0]['reason'], 'invalid_turn_records')
        packet = compile_evidence({'turn_records': [None, {'tool_calls': [None]}]})
        self.assertEqual(len(packet['source_errors']), 2)
        nested = {'id': 'dev-a', 'online': True}
        for _ in range(9):
            nested = {'data': nested}
        self.assertFalse(compile_calls(call(nested))['observations'])


if __name__ == '__main__':
    unittest.main()
