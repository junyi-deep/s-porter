FROM ubuntu:24.04

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        netcat-openbsd \
        openssh-server \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --create-home --shell /bin/bash tester \
    && echo "tester:tester123" | chpasswd \
    && echo "root:root123" | chpasswd \
    && mkdir -p /run/sshd \
    && ssh-keygen -A

COPY sshd_config /etc/ssh/sshd_config

EXPOSE 22

CMD ["/usr/sbin/sshd", "-D", "-e"]
