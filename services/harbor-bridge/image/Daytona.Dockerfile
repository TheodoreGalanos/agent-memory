FROM python:3.13-slim
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server procps \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 1000 --create-home memory \
    && mkdir -p /workspace /run/sshd /run/memory-asp /opt/memory \
    && chown memory:memory /workspace && chmod 700 /run/memory-asp \
    && ssh-keygen -A
COPY asp_helper.py /opt/memory/asp_helper.py
COPY interpreter.py /opt/memory/interpreter.py
COPY lease_watchdog.py /opt/memory/lease_watchdog.py
RUN chmod 644 /opt/memory/*.py
WORKDIR /workspace
