FROM python:3.13-slim
RUN apt-get update && apt-get install -y --no-install-recommends openssh-server procps tini iptables \
    && rm -rf /var/lib/apt/lists/* \
    && passwd -d root \
    && useradd --uid 1000 --create-home memory \
    && mkdir -p /workspace /run/sshd /run/memory-asp /root/.ssh /opt/memory \
    && chown memory:memory /workspace \
    && chmod 700 /run/memory-asp /root/.ssh
COPY asp_helper.py /opt/memory/asp_helper.py
COPY interpreter.py /opt/memory/interpreter.py
COPY image/lease_watchdog.py /opt/memory/lease_watchdog.py
COPY image/sshd_config /etc/ssh/sshd_config
COPY image/entrypoint.sh /opt/memory/entrypoint.sh
RUN chmod 755 /opt/memory/entrypoint.sh && chmod 644 /opt/memory/asp_helper.py
WORKDIR /workspace
ENTRYPOINT ["/usr/bin/tini", "--", "/opt/memory/entrypoint.sh"]
