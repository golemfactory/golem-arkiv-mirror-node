#!/usr/bin/env python3
"""The regular Arkiv Chain Indexer + UDC SDK against a disposable prefilled native chain.

Runs the arkiv-chain-indexer checkout's own, unmodified docker-compose.yml
(scanner, backfill scanner, gap filler, released UDC decoder, PostgreSQL,
Redis, aggregators, statistics, backend and frontend, built from that
checkout) against an isolated producer, drives the chain with the UDC SDK
smoke (scripts/udc-sdk-smoke.ts, exercise stage) and checks what the indexer
stored and projects. Configuration goes through the environment and an empty
env file in a temporary directory; nothing in the indexer checkout is written.

Requires Docker Compose, Bun, a release arkiv-proposer, and an arkiv-sdk-js
checkout that scripts/udc-sdk-smoke.py supports (dependencies installed,
`bun run build` done). Environment:

  ARKIV_INDEXER_DIR   arkiv-chain-indexer checkout
                      (default ../../arkiv-network/arkiv-chain-indexer)
  ARKIV_SDK_DIR       arkiv-sdk-js checkout (default ../arkiv-sdk-js)
  ARKIV_PROPOSER_BIN  node binary (default target/release/arkiv-proposer)

The containers reach the node through host.docker.internal, so its public
listener binds to the Docker default bridge gateway (the host side of
docker0, never a public interface); the private control listener stays on
loopback. Uses only disposable fixture keys and a randomly named compose
project whose containers, volumes and built images are removed afterwards.
"""
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
INDEXER = Path(os.environ.get('ARKIV_INDEXER_DIR', ROOT.parent.parent / 'arkiv-network' / 'arkiv-chain-indexer'))
SDK = Path(os.environ.get('ARKIV_SDK_DIR', ROOT.parent / 'arkiv-sdk-js'))
BINARY = Path(os.environ.get('ARKIV_PROPOSER_BIN', ROOT / 'target/release/arkiv-proposer'))
CHAIN_ID = '9405'


def main():
    spec = importlib.util.spec_from_file_location('fixture', ROOT / 'scripts/genesis-bundle-smoke.py')
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    assert (INDEXER / 'docker-compose.yml').is_file(), f'no docker-compose.yml in {INDEXER}; set ARKIV_INDEXER_DIR'
    assert (SDK / 'node_modules/viem').is_dir(), f'install the SDK dependencies in {SDK}; set ARKIV_SDK_DIR'
    assert shutil.which('bun') and BINARY.is_file(), 'install Bun and build the release arkiv-proposer first'
    gateway = subprocess.run(['docker', 'network', 'inspect', 'bridge', '-f', '{{(index .IPAM.Config 0).Gateway}}'],
                             check=True, capture_output=True, text=True).stdout.strip()
    reserved = set()

    def port(host='127.0.0.1'):
        while True:
            with socket.socket() as sock:
                sock.bind((host, 0))
                value = sock.getsockname()[1]
            if value not in reserved:
                reserved.add(value)
                return value

    def request(url, body=None, token=None):
        headers = {'Content-Type': 'application/json'}
        if token:
            headers['Authorization'] = 'Bearer ' + token
        req = urllib.request.Request(url, None if body is None else json.dumps(body).encode(), headers)
        with urllib.request.urlopen(req, timeout=10) as response:
            return json.load(response)

    def wait(check, timeout=90):
        end = time.monotonic() + timeout
        last = None
        while time.monotonic() < end:
            try:
                result = check()
                if result:
                    return result
            except (OSError, ValueError, KeyError, TypeError) as error:
                last = error
            time.sleep(.25)
        raise AssertionError(f'Timed out: {last}')

    with tempfile.TemporaryDirectory(prefix='arkiv-full-indexer-') as directory:
        work = Path(directory)
        producer_port, private_port = port(gateway), port()
        rpc = f'http://{gateway}:{producer_port}/'
        private = f'http://127.0.0.1:{private_port}'
        container_rpc = f'http://host.docker.internal:{producer_port}/'
        token = secrets.token_hex(32)
        for name in ('rpc-keys', 'baseload-config'):
            (work / name).mkdir()
        (work / 'compose.env').write_text('')  # never read the checkout's own .env
        env = dict(
            os.environ,
            ARKIV_PROPOSER_TOKEN=token, SDK_RPC_URL=rpc, SDK_PRIVATE_URL=private,
            SDK_REPORT_FILE=str(work / 'report.json'),
            POSTGRES_USER='indexer', POSTGRES_DB='indexer', POSTGRES_PASSWORD=secrets.token_hex(24),
            HOST_GATEWAY_IP=gateway, SCANNER_RPC_FULL_NODE=container_rpc,
            # Parsed even though the rpc-proxy profile stays off.
            RPC_PROXY_UPSTREAM=container_rpc,
            ENTITY_QUERY_INDEX='true', ENTITY_INDEX_GENESIS='auto', ENTITY_INDEX_GENESIS_RPC=container_rpc,
            SAVE_TRANSACTION_DATA='true', SCANNER_TRACK_BALANCES='true',
            # The forward scanner starts at the head; the backfill scanner covers the
            # blocks before it, down to genesis. The producer pauses before the final
            # checks, so the head itself must count as safe.
            SCANNER_CONFIRMATION_DEPTH='0', SCANNER_POLL_MS='500', SCANNER_RETRY_MS='1000',
            SCANNER_DISABLE_BACKFILL='false', SCANNER_OLDEST_BACKFILL_BLOCK='0', SCANNER_BACKFILL_SLEEP_MS='10',
            AGGREGATE_INTERVAL_MS='2000', SENDER_AGGREGATE_INTERVAL_MS='2000', STATISTICS_INTERVAL_MS='5000',
            BACKEND_HOST='127.0.0.1', BACKEND_PORT=str(port()), FRONTEND_HOST='127.0.0.1', FRONTEND_PORT=str(port()),
            RPC_KEY_DIR=str(work / 'rpc-keys'), BASELOAD_CONFIG_DIR=str(work / 'baseload-config'),
            VITE_CHAIN_NAME='Arkiv', VITE_TOKEN_SYMBOL='GLM',
        )
        for name in ('COMPOSE_PROFILES', 'COMPOSE_FILE', 'COMPOSE_PROJECT_NAME', 'DATABASE_URL', 'REDIS_URL', 'DECODER_URL'):
            env.pop(name, None)
        api = 'http://127.0.0.1:' + env['BACKEND_PORT']
        compose = ['docker', 'compose', '-p', 'arkiv-indexer-smoke-' + secrets.token_hex(5),
                   '--project-directory', str(INDEXER), '--env-file', str(work / 'compose.env'),
                   '-f', str(INDEXER / 'docker-compose.yml')]

        def dc(*args, timeout=120):
            return subprocess.run(compose + list(args), env=env, check=True, capture_output=True, text=True, timeout=timeout)

        # The chain of scripts/udc-sdk-smoke.py: three long-lived prefills, 500 ms
        # blocks and a 200,000-unit per-block storage cap, which the exercise checks.
        source = fixture.FIXTURE.replace('block_period_ms = "5000"', 'block_period_ms = "500"')
        for old, new in ((4, 10000), (5, 10001), (6, 10002)):
            source = source.replace(f'expires_at_height = "{old}"', f'expires_at_height = "{new}"')
        profile = (ROOT / 'docs/examples/genesis-bundle/profiles/fixed-glm-v2.toml').read_text()
        assert 'max_new_storage_units_per_block = "1500000"' in profile
        (work / 'profile.toml').write_text(profile.replace('max_new_storage_units_per_block = "1500000"',
                                                           'max_new_storage_units_per_block = "200000"'))
        preset = 'profile = { preset = "arkiv-entity-v3/glm-v2@4" }'
        assert preset in source
        (work / 'chain.toml').write_text(source.replace(preset, 'profile = { path = "profile.toml" }'))
        (work / 'proposer.key').write_text('0x' + '01' * 32)
        (work / 'proposer.key').chmod(0o600)
        modules = work / 'node_modules'
        (modules / '@arkiv-network').mkdir(parents=True)
        (modules / '@arkiv-network/sdk').symlink_to(SDK, target_is_directory=True)
        (modules / 'viem').symlink_to(SDK / 'node_modules/viem', target_is_directory=True)
        shutil.copyfile(ROOT / 'scripts/udc-sdk-smoke.ts', work / 'smoke.ts')
        for args in [('genesis', 'lock', work / 'chain.toml'),
                     ('genesis', 'build', work / 'chain.toml', '--out', work / 'bundle', '--locked')]:
            subprocess.run([str(BINARY), *map(str, args)], env=env, check=True, capture_output=True)
        node_log = (work / 'node.log').open('w')
        node = subprocess.Popen([str(BINARY), 'producer', '--run-dir', str(work / 'producer'),
                                 '--key-file', str(work / 'proposer.key'), '--bundle', str(work / 'bundle'),
                                 '--listen', f'{gateway}:{producer_port}', '--private-listen',
                                 f'127.0.0.1:{private_port}', '--start', 'running'],
                                env=env, stdout=node_log, stderr=node_log)
        started = False
        try:
            wait(lambda: request(rpc + 'signed/v1/status'))
            started = True
            dc('up', '-d', '--build', '--wait', '--wait-timeout', '300', timeout=1800)
            wait(lambda: request(api + '/health')['ok'])
            print('PASS indexer stack built from', INDEXER, 'is healthy', flush=True)
            subprocess.run(['bun', str(work / 'smoke.ts'), 'exercise'], cwd=work, env=env, check=True, timeout=300)
            request(private + '/signed/v1/control', {'action': 'pause'}, token)
            head = int(request(rpc + 'signed/v1/status')['head']['height'])

            def projected():
                index = request(api + '/health')['features']['entityQueryIndex']
                return index['projectedThroughBlock'] is not None and int(index['projectedThroughBlock']) == head
            wait(projected, timeout=180)
            health = request(api + '/health')
            genesis = health['features']['entityQueryIndex']['genesis']
            assert genesis and genesis['status'] == 'done', genesis
            print('PASS standard backend imported genesis:', json.dumps(genesis), flush=True)

            def rpc_call(url, method, params):
                answer = request(url, {'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params})
                assert 'error' not in answer, answer
                return answer['result']

            def without_bytes(rows):
                # The index keeps a bytes attribute's size and keccak256, never its content.
                for row in rows:
                    for attribute in row.get('attributes', []):
                        if attribute['type'] == 'bytes':
                            attribute['value'] = None
                return sorted(rows, key=lambda row: row['key'])
            select = {'key': True, 'owner': True, 'creator': True, 'attributeSchema': True, 'attributes': True,
                      'createdAt': True, 'updatedAt': True, 'expiresAt': True, 'creationFlags': True}
            options = {'atBlock': hex(head), 'limit': hex(64), 'select': select}
            expected = rpc_call(rpc, 'arkiv_query', ['*', options])
            assert 'cursor' not in expected, 'more live entities than one page'
            expected = without_bytes(expected['data'])
            actual = without_bytes(rpc_call(api + '/shadow-rpc/experimental', 'arkiv_query', ['*', options])['data'])
            assert actual == expected, (actual, expected)
            assert rpc_call(api + '/shadow-rpc/experimental', 'arkiv_getEntityCount', [{'block': head}]) == len(expected)
            report = json.loads((work / 'report.json').read_text())
            assert {row['key'] for row in expected} >= {report['entityKey'], report['maxKey']}, report
            entity = request(api + '/entity/' + report['entityKey'])
            assert entity, entity
            tx_hash = next(tx for number in range(1, head + 1)
                           for tx in rpc_call(rpc, 'eth_getBlockByNumber', [hex(number), False])['transactions'])
            tx = request(api + '/transaction/' + tx_hash)
            assert tx, tx
            sql = "SELECT json_build_object('blocks', (SELECT count(*) FROM blocks), 'transactions', (SELECT count(*) FROM transactions), 'operations', (SELECT count(*) FROM transaction_operations), 'logs', (SELECT count(*) FROM transaction_logs), 'balances', (SELECT count(*) FROM account_balances));"
            psql = ('exec', '-T', 'postgres', 'psql', '-U', 'indexer', '-d', 'indexer', '-Atc')
            counts = wait(lambda: (lambda c: c if c['blocks'] == head + 1 else None)(json.loads(dc(*psql, sql).stdout)))
            assert all(counts[key] > 0 for key in ('transactions', 'operations', 'logs', 'balances')), counts
            print('PASS scanner/backfill/decoder/PostgreSQL/entity projector:', json.dumps(counts), flush=True)
            print('PASS all live entity metadata matches node (bytes values withheld by design), including prefills and SDK lifecycle', flush=True)
            aggregate_sql = "SELECT (SELECT count(*) FROM block_ranges) > 0 AND (SELECT count(*) FROM sender_stats) > 0;"
            wait(lambda: dc(*psql, aggregate_sql).stdout.strip() == 't')
            wait(lambda: request(api + '/statistics')['chain']['id'] == CHAIN_ID)
            ui = 'http://127.0.0.1:' + env['FRONTEND_PORT']
            assert request(ui + '/api/health')['features']['transactionData']
            with urllib.request.urlopen(ui, timeout=10) as response:
                assert b'Arkiv BlockExplorer' in response.read()
            print('PASS range/sender aggregators, statistics and standard frontend API proxy', flush=True)
            dc('restart', 'backend', 'scanner')
            wait(lambda: request(api + '/health')['ok'])
            actual = without_bytes(rpc_call(api + '/shadow-rpc/experimental', 'arkiv_query', ['*', options])['data'])
            assert actual == expected
            print('PASS standard indexer restart retains complete projected state', flush=True)
        except Exception:
            if started:
                try:
                    print(dc('logs', '--tail', '20', 'scanner', 'backfill-scanner', 'backend', 'decoder').stdout[-12000:])
                except subprocess.SubprocessError as error:
                    print('could not read compose logs:', error)
            print('node:', (work / 'node.log').read_text()[-4000:])
            raise
        finally:
            try:
                if started:  # only this randomly named disposable test project
                    dc('down', '-v', '--remove-orphans', '--rmi', 'local', timeout=300)
            finally:
                node.terminate()
                node.wait(timeout=15)
                node_log.close()


if __name__ == '__main__':
    main()
