FROM debian:trixie-slim
ARG TARGETARCH
COPY jetpack-${TARGETARCH} /usr/local/bin/jetpack
RUN chmod +x /usr/local/bin/jetpack
ENTRYPOINT ["jetpack"]
