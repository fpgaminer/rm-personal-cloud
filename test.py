import asyncio
import aiohttp
import uuid
from datetime import datetime, timezone
import os
import random
import string
import dateutil.parser
import json
import sqlite3
import jwt
import sys


async def main():
	async with aiohttp.ClientSession(raise_for_status=True) as session:
		admin_token = get_admin_token()

		# Test service discovery API
		host = await api_get_service(session, sys.argv[1], "foobox")
		
		# Use Admin API to get device code
		device_code = await get_device_code(session, host, admin_token)

		# Test NewDevice API
		device_token = await api_new_device(session, host, device_code, "deviceDesc1", "test1")
		
		# Test NewUser API
		user_token = await api_new_user(session, host, device_token)
		auth_headers = {"Authorization": f"Bearer {user_token}"}

		# Stress test
		await test_stress(session, host, auth_headers)
		return

		# Test that auth APIs are properly authed
		await test_authorization(host, device_token, user_token)

		# Start notifications test
		websocket_task = asyncio.create_task(websocket_watch(session, host, auth_headers))
		
		# Test file manipulation APIs
		state = []
		expected_events = []

		for _ in range(128):
			effect = random.choice(['new', 'mutate', 'move', 'delete'])

			if effect == 'new':
				file = {
					"id": str(uuid.uuid4()),
					"name": random_string(random.randrange(8, 20)),
					"data": None,
					"file_type": "CollectionType",
					"current_page": random.randrange(4),
					"bookmarked": random.randrange(2) == 0,
					"parent": random.choice([x['id'] for x in state] + [""]),
					"date": datetime.now(timezone.utc),
					"version": 1,
				}

				if random.random() > 0.5:
					file['file_type'] = "DocumentType"
					file['data'] = os.urandom(random.randrange(1, 128))
				
				if file['data'] is not None:
					await api_upload_file(session, host, auth_headers, file['id'], 1, file['data'])
				
				await api_update_metadata(session, host, auth_headers, file['id'], 1, date=file['date'], file_type=file['file_type'], name=file['name'], current_page=file['current_page'], bookmarked=file['bookmarked'], parent=file['parent'])

				state.append(file)
				expected_events.append({"event": "DocAdded", **file})
			elif effect == 'mutate' and len(state) > 0:
				prop = random.choice(["name", "current_page", "bookmarked", "data"])
				file = random.choice(state)

				if file['data'] is None and prop == 'data':
					continue
				
				file['version'] += 1
				file['date'] = datetime.now(timezone.utc)

				if prop == 'name':
					file['name'] = random_string(random.randrange(8, 20))
					await api_update_metadata(session, host, auth_headers, file['id'], file['version'], date=file['date'], name=file['name'])
				elif prop == 'current_page':
					file['current_page'] = random.randrange(100)
					await api_update_metadata(session, host, auth_headers, file['id'], file['version'], date=file['date'], current_page=file['current_page'])
				elif prop == 'bookmarked':
					file['bookmarked'] = not file['bookmarked']
					await api_update_metadata(session, host, auth_headers, file['id'], file['version'], date=file['date'], bookmarked=file['bookmarked'])
				elif prop == 'data':
					file['data'] = os.urandom(random.randrange(1, 128))
					await api_upload_file(session, host, auth_headers, file['id'], file['version'], file['data'])
					await api_update_metadata(session, host, auth_headers, file['id'], file['version'], date=file['date'])
				
				expected_events.append({"event": "DocAdded", **file})
			elif effect == 'move' and len(state) > 0:
				file = random.choice(state)
				parent = random.choice(state)

				if file == parent:
					continue
				
				file['date'] = datetime.now(timezone.utc)
				file['parent'] = parent['id']
				file['version'] += 1

				await api_update_metadata(session, host, auth_headers, file['id'], file['version'], date=file['date'], parent=file['parent'])

				expected_events.append({"event": "DocAdded", **file})
			elif effect == 'delete' and len(state) > 0:
				file = random.choice(state)

				await api_delete_file(session, host, auth_headers, file['id'], file['version'])

				state.remove(file)
				expected_events.append({"event": "DocDeleted", **file})
		
		# Now check and make sure the server's state matches what we expect
		server_state = await api_list_files(session, host, auth_headers)

		for server_file in server_state:
			file = [x for x in state if x['id'] == server_file['ID']]
			if len(file) == 0:
				continue
			file = file[0]

			assert file['version'] == server_file['Version']
			# dateutil doesn't handle fractional seconds, so we have to use a sloppy comparison
			assert (file['date'] - dateutil.parser.isoparse(server_file['ModifiedClient'])).total_seconds() < 1
			assert file['file_type'] == server_file['Type']
			assert file['name'] == server_file['VissibleName']
			assert file['current_page'] == server_file['CurrentPage']
			assert file['bookmarked'] == server_file['Bookmarked']
			assert file['parent'] == server_file['Parent']

			if file['file_type'] == 'DocumentType':
				server_data = await api_download_file(session, host, auth_headers, file['id'])
				assert file['data'] == server_data
			
			state.remove(file)
		
		assert len(state) == 0

		# Wait a moment to make sure we grab all notifications
		await asyncio.sleep(4.0)

		# Now verify that all received notifications match what's expected
		websocket_task.cancel()
		notifications = await websocket_task

		for event,notification in zip(expected_events,notifications):
			assert notification['bookmarked'] == str(event['bookmarked']).lower()
			assert notification['event'] == event['event']
			assert notification['id'] == event['id']
			assert notification['parent'] == event['parent']
			assert notification['type'] == event['file_type']
			assert int(notification['version']) == event['version']
			assert notification['vissibleName'] == event['name']


async def websocket_watch(session, host, auth_headers):
	events = []

	async with session.ws_connect(f"https://{host}/notifications/ws/json/1", headers=auth_headers) as ws:
		#print("DEBUG: WebSocket: Connected")

		try:
			async for msg in ws:
				#print(f"DEBUG: WebSocketMsg: {msg}")

				if msg.type == aiohttp.WSMsgType.ERROR:
					break
				elif msg.type == aiohttp.WSMsgType.TEXT:
					events.append(json.loads(msg.data)['message']['attributes'])
		except asyncio.CancelledError:
			return events


async def api_get_service(session, host, service_name):
	async with session.get(f"https://{host}/service/json/1/foobox") as resp:
		assert resp.status == 200
		return (await resp.json())['Host']


async def api_new_device(session, host, code, device_desc, device_id):
	req = {"code": code, "deviceDesc": device_desc, "deviceID": device_id}

	async with session.post(f"https://{host}/token/json/2/device/new", json=req) as resp:
		return await resp.text()


async def api_new_user(session, host, device_token):
	async with session.post(f"https://{host}/token/json/2/user/new", headers={"Authorization": f"Bearer {device_token}"}) as resp:
		return await resp.text()


async def api_update_metadata(session, host, auth_headers, id, version, date=None, file_type=None, name=None, current_page=None, bookmarked=None, parent=None):
	req = {
		"ID": id,
		"Version": version,
	}

	if date is not None: req['ModifiedClient'] = date.isoformat()
	if file_type is not None: req['Type'] = file_type
	if name is not None: req['VissibleName'] = name
	if current_page is not None: req['CurrentPage'] = current_page
	if bookmarked is not None: req['Bookmarked'] = bookmarked
	if parent is not None: req['Parent'] = parent

	#print(f"UpdateMetadataRequest: {req}")
	async with session.put(f"https://{host}/document-storage/json/2/upload/update-status", json=[req], headers=auth_headers) as resp:
		assert resp.status == 200
		j = await resp.json()
		assert j[0]['Success']


async def api_upload_file(session, host, auth_headers, id, version, data):
	req = [{"ID": id, "Version": version, "Type": "DocumentType"}]
	async with session.put(f"https://{host}/document-storage/json/2/upload/request", json=req, headers=auth_headers) as resp:
		assert resp.status == 200
		j = await resp.json()
		assert j[0]['Success']
		url = j[0]['BlobURLPut']
	
	async with session.put(url, data=data) as resp:
		assert resp.status == 200


async def api_list_files(session, host, auth_headers, id=None):
	url = f"https://{host}/document-storage/json/2/docs"

	if id is not None:
		url += f"?doc={id}&withBlob=true"
	
	async with session.get(url, headers=auth_headers) as resp:
		assert resp.status == 200

		return await resp.json()


async def api_download_file(session, host, auth_headers, id):
	url = (await api_list_files(session, host, auth_headers, id=id))[0]['BlobURLGet']

	async with session.get(url) as resp:
		assert resp.status == 200
		return await resp.read()


async def api_delete_file(session, host, auth_headers, id, version):
	req = [{"ID": id, "Version": version}]
	async with session.put(f"https://{host}/document-storage/json/2/delete", json=req, headers=auth_headers) as resp:
		assert resp.status == 200


async def test_authorization(host, device_token, user_token):
	"""Makes sure that all authenticated APIs return errors when auth fails."""
	bad_auth_headers = {"Authorization": f"Bearer {device_token}"}

	calls = [
		lambda session: api_new_device(session, host, "foobar", "foobar", "foobar"),
		lambda session: api_new_user(session, host, user_token),
		lambda session: api_list_files(session, host, bad_auth_headers),
		lambda session: api_upload_file(session, host, bad_auth_headers, "", 1, b""),
		lambda session: api_update_metadata(session, host, bad_auth_headers, "", 2, file_type="DocumentType"),
		lambda session: api_delete_file(session, host, bad_auth_headers, "", 1),
		lambda session: session.ws_connect(f"https://{host}/notifications/ws/json/1", headers=bad_auth_headers),
	]

	for call in calls:
		# Was getting weird glitches, so I create a fresh session for each request here
		async with aiohttp.ClientSession(raise_for_status=True) as session:
				try:
					await call(session)
					raise Exception()
				except aiohttp.client_exceptions.ClientResponseError as e:
					assert e.status==401, "Authenticated APIs should return a 401 error when given bad authentication"
				except:
					assert True==False, "Authenticated APIs should return a 401 error when given bad authentication"


async def test_stress(session, host, auth_headers):
	"""Test if the server can handle a lot of parallel requests without breaking.  Typically breakage here will occur because SQLITE_BUSY wasn't handled correctly in the server."""
	ids = [str(uuid.uuid4()) for _ in range(32)]

	await asyncio.gather(*[
		api_upload_file(session, host, auth_headers, file_id, 1, b"")
		for file_id in ids
	])

	await asyncio.gather(*[
		api_update_metadata(session, host, auth_headers, file_id, 1, date=datetime.now(timezone.utc), file_type="DocumentType", name=random_string(16))
		for file_id in ids
	])

	await asyncio.gather(*[
		api_delete_file(session, host, auth_headers, file_id, 1)
		for file_id in ids
	])


def random_string(length):
	return ''.join(random.choice(string.digits + string.punctuation + string.ascii_letters) for i in range(length))


def get_admin_token():
	conn = sqlite3.connect('test.sqlite')
	secret = conn.execute("SELECT value FROM config WHERE key='jwt_secret_key'").fetchone()[0]
	secret = bytes.fromhex(secret)

	return jwt.encode({"sub": "Admin Token"}, secret, algorithm="HS256")


async def get_device_code(session, host, admin_token):
	async with session.post(f"https://{host}/admin/new_device_code", headers={"Authorization": f"Bearer {admin_token}"}) as resp:
		return (await resp.json())['code']


asyncio.run(main())