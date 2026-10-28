"""A horizon-server for a test's client: the world runs there, the client joins it.

The client has no world of its own (docs/crates.md). `horizon-ctl launch --world` does the same for
scripts that drive the game through horizon-ctl; this is for those that start horizon-client
themselves: the server gets a scratch folder, an offline `server.properties` (everyone welcome,
nothing protected, flight allowed, never paused), the player as an operator, and a control socket
the client is given as `--server-control` (what `horizon-ctl give`, `time`, `mobs`... ask).
"""
import hashlib
import json
import socket
import subprocess
import time
import uuid
from pathlib import Path


def offline_uuid(name):
    """UUIDUtil.createOfflinePlayerUUID: a name-based (version 3) MD5 UUID."""
    digest = bytearray(hashlib.md5(f'OfflinePlayer:{name}'.encode()).digest())
    digest[6] = digest[6] & 0x0f | 0x30
    digest[8] = digest[8] & 0x3f | 0x80
    return str(uuid.UUID(bytes=bytes(digest)))


def free_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1', 0))
        return s.getsockname()[1]


class Server:
    """horizon-server on `world` (under `universe`), in `folder`; `port` and `control` once started."""

    def __init__(self, binary, folder, resources, world='world', universe=None, seed=None, creative=False,
                 player='Player', distance=12, extra=(), env=None, difficulty='normal'):
        self.folder = Path(folder)
        self.folder.mkdir(parents=True, exist_ok=True)
        self.port = free_port()
        self.control = self.folder / 'control.sock'
        self.control.unlink(missing_ok=True)
        (self.folder / 'eula.txt').write_text('eula=true\n')
        (self.folder / 'server.properties').write_text(
            f'server-port={self.port}\nlevel-name={world}\nlevel-seed={"" if seed is None else seed}\n'
            f'gamemode={"creative" if creative else "survival"}\ndifficulty={difficulty}\n'
            'online-mode=false\nwhite-list=false\nenforce-secure-profile=false\nspawn-protection=0\n'
            f'allow-flight=true\nview-distance={distance}\nsimulation-distance={distance}\n'
            'pause-when-empty-seconds=0\nmotd=Horizon test\n')
        (self.folder / 'ops.json').write_text(json.dumps([{
            'uuid': offline_uuid(player), 'name': player, 'level': 4, 'bypassesPlayerLimit': False}]))
        command = [str(binary), '--resources', str(resources), '--control-socket', str(self.control),
                   *map(str, extra), '--nogui']
        if universe is not None:
            command += ['--universe', str(universe)]
        self.log_path = self.folder / 'server.out'
        self.log = self.log_path.open('w')
        self.process = subprocess.Popen(command, cwd=self.folder, env=env, stdin=subprocess.DEVNULL,
                                        stdout=self.log, stderr=subprocess.STDOUT)

    def wait_started(self, timeout=600):
        deadline = time.time() + timeout
        while 'Done (' not in self.log_path.read_text(errors='replace'):
            if self.process.poll() is not None or time.time() > deadline:
                raise RuntimeError('horizon-server did not start:\n' + self.log_path.read_text(errors='replace')[-3000:])
            time.sleep(0.2)
        return self

    def client_args(self, player='Player'):
        """What the client joins this server with."""
        return ['--server', f'127.0.0.1:{self.port}', '--name', player, '--server-control', str(self.control)]

    def request(self, line):
        """One request on the server's control socket and its JSON reply."""
        with socket.socket(socket.AF_UNIX) as s:
            s.settimeout(60)
            s.connect(str(self.control))
            s.sendall((line + '\n').encode())
            return json.loads(s.makefile().readline())

    def stop(self, timeout=120):
        """`stop` on the console: the world saves, then the server exits."""
        if self.process.poll() is None:
            try:
                self.request('console stop')
            except OSError:
                pass
            try:
                self.process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.log.close()
