"""Lock the open file description inherited from the worker as descriptor 3.

The worker keeps its descriptor open, so the lock survives this helper's exit and
is released by the kernel when the worker closes it or dies.
"""

import fcntl

try:
    fcntl.flock(3, fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    raise SystemExit(2) from None
