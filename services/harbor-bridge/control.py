"""Protected endpoint control shared by the Docker and Daytona owners."""

import shlex


async def renew_binding(env, record, expires_at):
    # The lifecycle owner serializes this with teardown and checks the host lease.
    script = "import json,pathlib; p=pathlib.Path('/run/memory-asp/binding.json'); v=json.loads(p.read_text()); "
    script += f"assert v['epoch']=={record['request']['assignment']['epoch']}; v['expires_at']={expires_at}; t=p.with_suffix('.next'); t.write_text(json.dumps(v)); t.replace(p)"
    result = await env.exec(
        "python3 -c " + shlex.quote(script), user="root", timeout_sec=10
    )
    if result.return_code:
        raise RuntimeError("Sandbox binding renewal failed")


async def export_file(env, path, destination):
    script = (
        "import pathlib,sys; p=pathlib.Path("
        + repr(path)
        + "); sys.exit(44) if not p.exists() else None; p=p.resolve(strict=True); assert p.is_relative_to('/workspace') and p.is_file(); assert p.stat().st_size<=16*1024*1024; print(p)"
    )
    result = await env.exec(
        "python3 -c " + shlex.quote(script), user=1000, timeout_sec=10
    )
    if result.return_code == 44:
        raise FileNotFoundError("Declared sandbox output was not produced")
    if result.return_code:
        raise RuntimeError(
            "Export must be a readable workspace file no larger than 16 MiB"
        )
    await env.download_file(result.stdout.strip(), destination)
    if (
        destination.is_symlink()
        or not destination.is_file()
        or destination.stat().st_size > 16 * 1024 * 1024
    ):
        raise RuntimeError("Invalid downloaded artifact")


async def fence_processes(env, record):
    await renew_binding(env, record, 0)
    result = await env.exec(
        "pkill -KILL -u 1000 || test $? -eq 1", user="root", timeout_sec=10
    )
    if result.return_code:
        raise RuntimeError("Generated processes could not be stopped")
