"""Persistent Python state inside an already scoped sandbox, never on the host.

Only explicitly selected JSON objects are checkpoints. Execution receipts survive
interpreter loss; an unfinished request is uncertain and is not replayed.
"""

import contextlib
import fcntl
import io
import json
import multiprocessing
import os
import resource
import socket
import subprocess
import sys
import threading
import time
import uuid
from pathlib import Path

MAX_BYTES = 65536


def encode(value):
    data = json.dumps(value, allow_nan=False).encode()
    if len(data) > MAX_BYTES:
        raise ValueError("Interpreter message exceeds 64 KiB")
    return data


def save(path, value):
    data = encode(value)
    temporary = path.with_suffix(".tmp")
    temporary.write_bytes(data)
    temporary.replace(path)


def receive(stream):
    data = bytearray()
    while b"\n" not in data:
        chunk = stream.recv(min(4096, MAX_BYTES + 1 - len(data)))
        if not chunk:
            raise RuntimeError("Interpreter disconnected; execution may be unknown")
        data.extend(chunk)
        if len(data) > MAX_BYTES:
            raise ValueError("Interpreter message exceeds 64 KiB")
    return json.loads(data.split(b"\n", 1)[0])


def directory(workspace, identifier):
    if str(uuid.UUID(identifier)) != identifier:
        raise ValueError("Invalid interpreter identity")
    # Short socket paths also work on macOS development fixtures.
    return Path(workspace) / (".repl-" + identifier)


def request(workspace, command):
    root = directory(workspace, command["session_id"])
    root.mkdir(mode=0o700, exist_ok=True)
    op = command["action"]
    if op == "start":
        with (root / "start.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            return start(workspace, root, command)
    if op == "receipt":
        receipt = root / (str(uuid.UUID(command["request_id"])) + ".json")
        return (
            json.loads(receipt.read_text())
            if receipt.exists()
            else {"status": "unknown"}
        )
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
        stream.settimeout(8)
        try:
            stream.connect(str(root / "socket"))
        except (FileNotFoundError, ConnectionRefusedError) as error:
            # No automatic execution replay or inferred heap recovery.
            raise RuntimeError(
                "Interpreter lost; restart and restore an explicit checkpoint"
            ) from error
        stream.sendall(encode(command) + b"\n")
        reply = receive(stream)
        if op == "stop":
            until = time.monotonic() + 3
            while (root / "socket").exists() and time.monotonic() < until:
                time.sleep(0.01)
        return reply


def start(workspace, root, command):
    duration = command.get("lifetime_seconds", 300)
    idle = command.get("idle_seconds", 30)
    if not 1 <= duration <= 900 or not 1 <= idle <= duration:
        raise ValueError("Invalid interpreter lifetime")
    if (root / "socket").exists():
        try:
            return request(workspace, {**command, "action": "inspect"})
        except RuntimeError:
            # The supervisor is gone. Starting a fresh heap never replays code.
            (root / "socket").unlink(missing_ok=True)
    generation = str(uuid.uuid4())
    env = {
        key: os.environ[key] for key in ("PATH", "LANG", "LC_ALL") if key in os.environ
    }
    server = subprocess.Popen(
        [
            sys.executable,
            str(Path(__file__).resolve()),
            "--serve",
            str(root),
            generation,
            str(duration),
            str(idle),
        ],
        cwd=workspace,
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
        close_fds=True,
    )
    threading.Thread(target=server.wait, daemon=True).start()
    until = time.monotonic() + 3
    while time.monotonic() < until:
        if server.returncode is not None:
            raise RuntimeError(
                f"Interpreter failed to start (exit {server.returncode})"
            )
        if (root / "socket").exists():
            return request(workspace, {**command, "action": "inspect"})
        time.sleep(0.02)
    raise RuntimeError("Interpreter failed to start")


class BoundedOutput(io.StringIO):
    def write(self, value):
        if len(self.getvalue().encode()) + len(value.encode()) > 8192:
            raise ValueError(
                "Interpreter output exceeds 8 KiB; write a result artifact"
            )
        return super().write(value)


def evaluate(command, namespace, generation):
    response = {"status": "ok", "generation": generation}
    try:
        action = command["action"]
        if action == "execute":
            if len(command["code"].encode()) > 32768:
                raise ValueError("Code exceeds 32 KiB")
            output = BoundedOutput()
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                exec(
                    compile(command["code"], "<scoped-interpreter>", "exec"), namespace
                )
            response["output"] = output.getvalue()
        elif action == "checkpoint":
            names = command["names"]
            if len(names) > 128 or any(
                not isinstance(n, str) or n.startswith("__") for n in names
            ):
                raise ValueError("Select named application objects")
            response["objects"] = {name: namespace[name] for name in names}
        elif action == "restore":
            objects = command["objects"]
            if not isinstance(objects, dict) or any(
                k.startswith("__") for k in objects
            ):
                raise ValueError("Invalid application checkpoint")
            namespace.update(objects)
            response["restored"] = list(objects)
        else:
            raise ValueError("Unsupported interpreter operation")
        encode(response)
    except BaseException as error:
        response = {
            "status": "failed",
            "generation": generation,
            "error": f"{type(error).__name__}: {str(error)[:512]}",
            "effects": "Code may have changed objects or sandbox files before failure",
        }
    return response


def worker(channel, generation, duration):
    resource.setrlimit(resource.RLIMIT_CPU, (duration, duration + 1))
    if sys.platform == "linux":
        resource.setrlimit(resource.RLIMIT_AS, (512 * 1024 * 1024, 512 * 1024 * 1024))
    namespace = {"__name__": "__memory_interpreter__"}
    while True:
        command = json.loads(channel.recv_bytes(MAX_BYTES))
        channel.send_bytes(encode(evaluate(command, namespace, generation)))


def serve(root, generation, duration, idle):
    expires = time.monotonic() + duration
    root = Path(root)
    socket_path = root / "socket"
    socket_path.unlink(missing_ok=True)
    process_context = multiprocessing.get_context("fork")
    parent, child = process_context.Pipe()
    process = process_context.Process(target=worker, args=(child, generation, duration))
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.bind(str(socket_path))
        os.chmod(socket_path, 0o600)
        listener.listen(1)
        process.start()
        child.close()
        try:
            while time.monotonic() < expires:
                listener.settimeout(min(idle, max(0.01, expires - time.monotonic())))
                try:
                    client, _ = listener.accept()
                except TimeoutError:
                    break
                with client:
                    client.settimeout(5)
                    receipt = None
                    try:
                        command = receive(client)
                        action = command["action"]
                        response = {
                            "status": "ok" if process.is_alive() else "lost",
                            "generation": generation,
                        }
                        if action == "inspect":
                            client.sendall(encode(response) + b"\n")
                            continue
                        if command.get("generation") != generation:
                            raise ValueError(
                                "Interpreter generation changed; explicit restoration is required"
                            )
                        if action == "stop":
                            client.sendall(encode(response) + b"\n")
                            break
                        if not process.is_alive():
                            raise RuntimeError(
                                "Interpreter lost; restart and restore an explicit checkpoint"
                            )
                        if action == "execute":
                            identifier = str(uuid.UUID(command["request_id"]))
                            receipt = root / (identifier + ".json")
                            if receipt.exists():
                                client.sendall(
                                    encode(json.loads(receipt.read_text())) + b"\n"
                                )
                                continue
                            save(
                                receipt,
                                {
                                    "status": "unknown",
                                    "generation": generation,
                                    "request_id": identifier,
                                },
                            )
                        seconds = command.get("timeout_seconds", 3)
                        if not 1 <= seconds <= 5:
                            raise ValueError("Invalid execution timeout")
                        parent.send_bytes(encode(command))
                        if not parent.poll(
                            min(seconds, max(0.001, expires - time.monotonic()))
                        ):
                            process.kill()
                            process.join()
                            raise RuntimeError(
                                "Interpreter timed out; heap lost and effects are unknown"
                            )
                        response = json.loads(parent.recv_bytes(MAX_BYTES))
                        if receipt:
                            save(receipt, response)
                    except Exception as error:
                        response = {
                            "status": "unknown",
                            "generation": generation,
                            "error": f"{type(error).__name__}: {str(error)[:512]}",
                        }
                        if receipt:
                            save(receipt, response)
                    try:
                        client.sendall(encode(response) + b"\n")
                    except (BrokenPipeError, ConnectionResetError):
                        pass  # Receipt permits reconciliation after a lost reply.
        finally:
            if process.is_alive():
                process.kill()
            process.join(timeout=2)
            parent.close()
            socket_path.unlink(missing_ok=True)


if __name__ == "__main__":
    if len(sys.argv) != 6 or sys.argv[1] != "--serve":
        raise SystemExit("Private interpreter server")
    serve(sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5]))
