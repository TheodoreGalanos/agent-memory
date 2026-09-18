"""Bounded ASP exec fallback. Install outside the model-writable workspace.

The provisioner owns MEMORY_ASP_CONTROL and binding.json. This helper does not
create a security boundary by itself; provider users/mounts must protect them.
"""

import base64
import errno
import json
import os
import pwd
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from urllib.parse import unquote, urlsplit

MAX_MESSAGE = 2 * 1024 * 1024
MAX_FILE = 1024 * 1024
CONTROL = Path(os.environ.get("MEMORY_ASP_CONTROL", "/run/memory-asp"))


def save(path, data):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(data))
    temporary.replace(path)


def binding(request):
    current = json.loads((CONTROL / "binding.json").read_text())
    if any(
        request.get(key) != current[key]
        for key in ("sandbox_id", "generation", "session_id", "epoch")
    ):
        raise PermissionError("Sandbox ownership changed")
    if time.time() * 1000 >= current["expires_at"]:
        raise PermissionError("Sandbox binding expired")
    return current


def addressed(path, workspace, uid):
    if not isinstance(path, str) or "\0" in path:
        raise ValueError("Invalid path")
    if path == "~" or path.startswith("~/"):
        home = pwd.getpwuid(uid).pw_dir
        path = home if path == "~" else os.path.join(home, path[2:])
    elif path.startswith("file://"):
        url = urlsplit(path)
        if url.netloc in ("", "localhost") and "%2f" not in url.path.lower():
            path = unquote(url.path)
    return os.path.abspath(os.path.join(workspace, path))


def info(path):
    value = os.lstat(path)
    kind = (
        "symlink"
        if stat.S_ISLNK(value.st_mode)
        else "directory" if stat.S_ISDIR(value.st_mode) else "file"
    )
    return dict(
        name=os.path.basename(path),
        path=path,
        kind=kind,
        size=value.st_size,
        mtimeMs=value.st_mtime * 1000,
    )


def execution_dir(identifier):
    if (
        not isinstance(identifier, str)
        or len(identifier) > 80
        or not identifier
        or any(
            c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_"
            for c in identifier
        )
    ):
        raise ValueError("Invalid execution id")
    return CONTROL / "executions" / identifier


def serve(request):
    owner = binding(request)
    workspace = owner["workspace"]
    op = request["op"]
    if op not in ("start", "status", "cancel", "output", "spill") and os.getuid() == 0:
        os.setgroups([])
        os.setgid(owner["exec_gid"])
        os.setuid(owner["exec_uid"])
    if op == "interpreter":
        from interpreter import request as interpreter_request

        return interpreter_request(workspace, request["request"])
    path = addressed(request.get("path", "."), workspace, owner["exec_uid"])
    if op == "absolute":
        return path
    if op == "join":
        parts = request["parts"]
        if any(not isinstance(part, str) or "\0" in part for part in parts):
            raise ValueError("Invalid path component")
        return os.path.normpath("/".join(part for part in parts if part))
    if op in ("read", "read_chunk"):
        limit = min(MAX_FILE, request.get("limit", MAX_FILE))
        if limit < 1:
            raise ValueError("Invalid read limit")
        with open(path, "rb") as stream:
            stream.seek(request.get("offset", 0))
            data = stream.read(limit + (op == "read"))
        if len(data) > limit:
            raise ValueError("File exceeds the bounded read limit")
        return dict(data=base64.b64encode(data).decode(), eof=len(data) < limit)
    if op in ("write", "append"):
        data = base64.b64decode(request["data"], validate=True)
        if len(data) > MAX_FILE:
            raise ValueError("Write exceeds the bounded transfer limit")
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        with open(path, "ab" if op == "append" else "wb") as stream:
            stream.write(data)
        return None
    if op == "rename":
        os.replace(
            path, addressed(request["destination"], workspace, owner["exec_uid"])
        )
        return None
    if op == "info":
        return info(path)
    if op == "list":
        result = []
        with os.scandir(path) as entries:
            for entry in entries:
                if len(result) >= 4096:
                    raise ValueError("Directory listing exceeds 4096 entries")
                result.append(info(entry.path))
        return result
    if op == "canonical":
        return str(Path(path).resolve(strict=True))
    if op == "exists":
        try:
            os.stat(path)
            return True
        except FileNotFoundError:
            return False
    if op == "mkdir":
        Path(path).mkdir(
            parents=request.get("recursive", True),
            exist_ok=request.get("recursive", True),
        )
        return None
    if op == "remove":
        try:
            if Path(path).is_dir() and not Path(path).is_symlink():
                if request.get("recursive", False):
                    shutil.rmtree(path)
                else:
                    os.rmdir(path)
            else:
                os.unlink(path)
        except FileNotFoundError:
            if not request.get("force", False):
                raise
        return None
    if op in ("temp_file", "temp_dir"):
        # Prefixes and suffixes are names, never caller-selected directories.
        prefix, suffix = request.get("prefix", "tmp-"), request.get("suffix", "")
        if any("/" in part or "\0" in part for part in (prefix, suffix)):
            raise ValueError("Invalid temporary name")
        if op == "temp_dir":
            return tempfile.mkdtemp(prefix=prefix, dir=workspace)
        fd, name = tempfile.mkstemp(prefix=prefix, suffix=suffix, dir=workspace)
        os.close(fd)
        return name
    if op == "start":
        if (
            not 1 <= request["max_output_bytes"] <= 16 * MAX_FILE
            or not 1 <= request["timeout_ms"] <= 24 * 60 * 60 * 1000
        ):
            raise ValueError("Execution exceeds helper resource limits")
        directory = execution_dir(request["execution_id"])
        directory.mkdir(parents=True, exist_ok=False)
        request["cwd"] = addressed(
            request.get("cwd", "."), workspace, owner["exec_uid"]
        )
        request["deadline"] = min(
            owner["expires_at"], time.time() * 1000 + request["timeout_ms"]
        )
        save(directory / "request.json", request)
        save(directory / "status.json", {"state": "starting"})
        subprocess.Popen(
            [
                sys.executable,
                str(Path(__file__).resolve()),
                "--run",
                request["execution_id"],
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
            close_fds=True,
        )
        return {"state": "starting"}
    if op in ("status", "cancel", "output", "spill"):
        directory = execution_dir(request["execution_id"])
        original = json.loads((directory / "request.json").read_text())
        if any(
            request[key] != original[key]
            for key in ("sandbox_id", "generation", "session_id", "epoch")
        ):
            raise PermissionError("Execution belongs to another assignment")
        if op == "spill":
            status = json.loads((directory / "status.json").read_text())
            if status["state"] != "finished":
                raise ValueError("Output has not settled")
            # Only the output copy is readable by generated code. Control records
            # and endpoint keys remain in the protected control directory.
            destination = Path(
                tempfile.mkdtemp(prefix="memory-asp-output-", dir=CONTROL.parent)
            )
            output = destination / "output"
            shutil.copyfile(directory / "output", output)
            output.chmod(0o444)
            destination.chmod(0o755)
            return str(output)
        if op == "cancel":
            (directory / "cancel").touch()
        if op == "output":
            with (directory / "output").open("rb") as stream:
                stream.seek(request.get("offset", 0))
                return base64.b64encode(
                    stream.read(min(MAX_FILE, request.get("limit", MAX_FILE)))
                ).decode()
        return json.loads((directory / "status.json").read_text())
    raise ValueError("Unsupported operation")


def run_execution(identifier):
    directory = execution_dir(identifier)
    request = json.loads((directory / "request.json").read_text())
    process = None
    try:
        owner = binding(request)
        environment = (
            {
                key: os.environ[key]
                for key in ("PATH", "LANG", "LC_ALL", "TERM")
                if key in os.environ
            }
            if request.get("inherit_env", True)
            else {}
        )
        # Control paths and transport identity are never passed to generated code.
        environment.update(request.get("env", {}))
        environment.pop("MEMORY_ASP_CONTROL", None)
        identity = (
            {"user": owner["exec_uid"], "group": owner["exec_gid"], "extra_groups": []}
            if os.getuid() == 0
            else {}
        )
        process = subprocess.Popen(
            ["/bin/sh", "-c", request["command"]],
            cwd=request["cwd"],
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            stdin=subprocess.DEVNULL,
            start_new_session=True,
            **identity,
        )
        state = {
            "state": "running",
            "pid": process.pid,
            "started_at": time.time() * 1000,
            "bytes": 0,
            "output_path": str(directory / "output"),
        }
        save(directory / "status.json", state)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        reason = None
        killed_at = None
        with (directory / "output").open("wb") as output:
            # Closing stdout does not settle a process or remove cancellation duties.
            while selector.get_map() or process.poll() is None:
                if reason is None:
                    if (directory / "cancel").exists():
                        reason = "cancelled"
                    elif time.time() * 1000 >= request["deadline"]:
                        reason = "timeout"
                    else:
                        try:
                            binding(request)
                        except (PermissionError, FileNotFoundError):
                            reason = "ownership_lost"
                    if reason:
                        try:
                            os.killpg(process.pid, signal.SIGTERM)
                        except ProcessLookupError:
                            pass
                        killed_at = time.monotonic()
                if killed_at is not None and time.monotonic() - killed_at > 0.5:
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                for key, _ in selector.select(0.05):
                    chunk = os.read(key.fd, 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        break
                    remaining = max(0, request["max_output_bytes"] - state["bytes"])
                    output.write(chunk[:remaining])
                    output.flush()
                    state["bytes"] += len(chunk[:remaining])
                    if len(chunk) > remaining and reason is None:
                        reason = "output_limit"
                        killed_at = time.monotonic()
                        try:
                            os.killpg(process.pid, signal.SIGTERM)
                        except ProcessLookupError:
                            pass
        selector.close()
        process.stdout.close()
        exit_code = process.wait()
        # Descendants that closed stdout still belong to this execution group.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        gone = False
        for _ in range(20):
            try:
                os.killpg(process.pid, 0)
            except ProcessLookupError:
                gone = True
                break
            time.sleep(0.05)
        save(
            directory / "status.json",
            {
                **state,
                "state": "finished" if gone else "outcome_unknown",
                "exit_code": exit_code,
                "reason": reason,
                "process_group_gone": gone,
                "finished_at": time.time() * 1000,
            },
        )
    except Exception as error:
        if process:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        save(
            directory / "status.json",
            {"state": "outcome_unknown", "reason": type(error).__name__},
        )


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--run":
        run_execution(sys.argv[2])
        return
    try:
        raw = sys.stdin.buffer.read(MAX_MESSAGE + 1)
        if len(raw) > MAX_MESSAGE:
            raise ValueError("Request exceeds transfer limit")
        result = {"ok": True, "value": serve(json.loads(raw))}
    except Exception as error:
        codes = {
            errno.ENOENT: "not_found",
            errno.EACCES: "permission_denied",
            errno.EPERM: "permission_denied",
            errno.ENOTDIR: "not_directory",
            errno.EISDIR: "is_directory",
        }
        code = (
            "invalid"
            if isinstance(error, (ValueError, KeyError, TypeError))
            else codes.get(getattr(error, "errno", None), "unknown")
        )
        if isinstance(error, PermissionError):
            code = "permission_denied"
        result = {"ok": False, "error": {"code": code, "message": str(error)}}
    encoded = json.dumps(result).encode()
    if len(encoded) > MAX_MESSAGE:
        encoded = b'{"ok":false,"error":{"code":"invalid","message":"Response exceeds transfer limit"}}'
    sys.stdout.buffer.write(encoded)


if __name__ == "__main__":
    main()
