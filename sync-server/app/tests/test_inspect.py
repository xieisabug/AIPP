from app.tests.test_sync_api import make_client, auth_headers, sample_event
from app.db import get_sessionmaker
from app.models import SyncAccount, SyncToken, SyncDevice
from datetime import UTC, datetime
from hashlib import sha256


def test_inspect_empty_and_no_device_registration():
    client = make_client()
    assert client.post('/v1/sync/inspect', json={'sequences': []}).status_code == 401
    result = client.post('/v1/sync/inspect', headers=auth_headers(), json={'sequences': []})
    assert result.status_code == 200
    assert result.json()['remote_empty'] is True
    with get_sessionmaker()() as db:
        assert db.query(SyncDevice).count() == 0


def test_inspect_history_not_latest_object_and_legacy_sync_unchanged():
    client = make_client()
    first = client.post('/v1/sync/push', headers=auth_headers(), json={'device_id':'d', 'events':[sample_event('e1','c1')]}).json()
    seq = first['accepted'][0]['server_seq']
    event = sample_event('e2','c1',1)
    event['payload']['fields']['name'] = 'updated'
    client.post('/v1/sync/push', headers=auth_headers(), json={'device_id':'d','events':[event]})
    result = client.post('/v1/sync/inspect',headers=auth_headers(),json={'sequences':[seq]}).json()
    assert result['changes'][0]['payload']['fields']['name'] == '需求讨论'
    assert result['latest_cursor'] > seq
    assert result['remote_empty'] is False
    # Original clients continue to use exactly the same endpoints/schema.
    pulled = client.get('/v1/sync/pull?cursor=0', headers=auth_headers(device_id='reader')).json()
    assert len(pulled['changes']) == 2
    assert client.get('/v1/sync/status',headers=auth_headers()).json()['object_count'] == 1


def test_inspect_account_isolation_and_limit():
    client = make_client()
    client.post('/v1/sync/push', headers=auth_headers(),json={'device_id':'d','events':[sample_event('e','c')]})
    with get_sessionmaker()() as db:
        db.add(SyncAccount(id='b',created_at=datetime.now(UTC)))
        db.add(SyncToken(id='bt',account_id='b',name='b',token_hash=sha256(b'other').hexdigest(),created_at=datetime.now(UTC)))
        db.commit()
    result = client.post('/v1/sync/inspect',headers=auth_headers('other'),json={'sequences':[1]}).json()
    assert result['changes'] == [] and result['remote_empty'] is True
    assert client.post('/v1/sync/inspect',headers=auth_headers(),json={'sequences':list(range(11))}).status_code == 422


def test_inspect_tombstones_are_not_empty():
    client = make_client()
    e = sample_event('delete','gone')
    e.update(operation='delete', payload=None)
    client.post('/v1/sync/push',headers=auth_headers(),json={'device_id':'d','events':[e]})
    assert client.post('/v1/sync/inspect',headers=auth_headers(),json={'sequences':[]}).json()['remote_empty'] is False

def test_inspect_returns_public_key_check_config():
    import base64
    import json
    client = make_client()
    key_id = 'natural:' + base64.b64encode(b'data_sync_secret\x1fkey_check').decode()
    event = sample_event('key-event',key_id)
    event['object_type'] = 'system.feature_config'
    event['payload'] = {'fields':{'feature_code':'data_sync_secret','key':'key_check','value':json.dumps({'v':1,'salt':'public-salt','check':'public-check'})},'refs':{}}
    assert client.post('/v1/sync/push',headers=auth_headers(),json={'device_id':'d','events':[event]}).json()['accepted']
    result = client.post('/v1/sync/inspect',headers=auth_headers(),json={'sequences':[]}).json()
    assert result['key_check'] == event['payload']
