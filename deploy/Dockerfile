# Image du studio, sans étape de compilation : le binaire est compilé sur l'hôte (`deploy/build.sh`),
# le build Angular aussi. Même glibc que l'hôte (Ubuntu 26.04), sinon le binaire ne démarre pas.
FROM ubuntu:26.04

RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg libfontconfig1 ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=bin dijkstravel-server /app/bin/dijkstravel-server
COPY --from=web . /app/web

ENV WEB_DIR=/app/web CACHE_DIR=/cache SCRATCH_DIR=/work LISTEN_ADDR=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/app/bin/dijkstravel-server"]
CMD ["api"]
