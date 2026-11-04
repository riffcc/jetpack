FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends openssh-client ca-certificates \
    && rm -rf /var/lib/apt/lists/*
ARG TARGETARCH
COPY jetpack-${TARGETARCH} /usr/local/bin/jetpack
RUN chmod +x /usr/local/bin/jetpack
ENTRYPOINT ["jetpack"]
